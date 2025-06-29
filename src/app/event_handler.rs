use crate::app::build::WaylandBackend;
use crate::app::event_centralizer::CentralizedEvent;
use crate::utils::logging::PolarBearExpectation;
use crate::wayland::compositor::{send_frames_surface_tree, ClientState, State};
use crate::wayland::element::WindowElement;
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, Event, InputEvent, KeyboardKeyEvent, PointerAxisEvent,
    PointerButtonEvent, TouchEvent,
};
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::utils::draw_render_elements;
use smithay::backend::renderer::{Color32F, Frame, Renderer};
use smithay::desktop::Space;
use smithay::input::keyboard::FilterResult;
use smithay::input::{pointer, touch};
use smithay::reexports::wayland_server::protocol::wl_pointer::ButtonState;
use smithay::utils::{Logical, Point, Rectangle, Transform, SERIAL_COUNTER};
use smithay::wayland::shell::xdg::ToplevelSurface;
use std::sync::Arc;
use winit::event_loop::ActiveEventLoop;

/**
 * As we currently use Xwayland, there is only 1 surface
 */
fn get_surface(state: &State) -> Option<ToplevelSurface> {
    state
        .xdg_shell_state
        .toplevel_surfaces()
        .iter()
        .next()
        .cloned()
}

fn clamp_coords(space: &Space<WindowElement>, pos: Point<f64, Logical>) -> Point<f64, Logical> {
    if space.outputs().next().is_none() {
        return pos;
    }

    let (pos_x, pos_y) = pos.into();
    let max_x = space
        .outputs()
        .fold(0, |acc, o| acc + space.output_geometry(o).unwrap().size.w);
    let clamped_x = pos_x.clamp(0.0, max_x as f64);
    let max_y = space
        .outputs()
        .find(|o| {
            let geo = space.output_geometry(o).unwrap();
            geo.contains((clamped_x as i32, 0))
        })
        .map(|o| space.output_geometry(o).unwrap().size.h);

    if let Some(max_y) = max_y {
        let clamped_y = pos_y.clamp(0.0, max_y as f64);
        (clamped_x, clamped_y).into()
    } else {
        (clamped_x, pos_y).into()
    }
}

pub fn handle(event: CentralizedEvent, backend: &mut WaylandBackend, event_loop: &ActiveEventLoop) {
    match event {
        CentralizedEvent::CloseRequested => {
            log::info!("The close button was pressed; stopping");
            event_loop.exit();
        }
        CentralizedEvent::Redraw => {
            if let Some(winit) = backend.graphic_renderer.as_mut() {
                let size = winit.window_size();
                let damage = Rectangle::from_size(size);
                {
                    let (renderer, mut framebuffer) = winit.bind().unwrap();

                    let compositor = &mut backend.compositor;

                    let elements = compositor
                        .state
                        .xdg_shell_state
                        .toplevel_surfaces()
                        .iter()
                        .flat_map(|surface| {
                            render_elements_from_surface_tree(
                                renderer,
                                surface.wl_surface(),
                                (0, 0),
                                1.0,
                                1.0,
                                Kind::Unspecified,
                            )
                        })
                        .collect::<Vec<WaylandSurfaceRenderElement<GlesRenderer>>>();

                    let mut frame = renderer
                        .render(&mut framebuffer, size, Transform::Flipped180)
                        .unwrap();
                    frame
                        .clear(Color32F::new(0.1, 0.0, 0.0, 1.0), &[damage])
                        .unwrap();
                    draw_render_elements(&mut frame, 1.0, &elements, &[damage]).unwrap();
                    // We rely on the nested compositor to do the sync for us
                    let _ = frame.finish().unwrap();

                    for surface in compositor.state.xdg_shell_state.toplevel_surfaces() {
                        send_frames_surface_tree(
                            surface.wl_surface(),
                            compositor.start_time.elapsed().as_millis() as u32,
                        );
                    }

                    if let Some(stream) = compositor
                        .listener
                        .accept()
                        .pb_expect("Failed to accept listener")
                    {
                        log::info!("Got a client: {:?}", stream);

                        let client = compositor
                            .display
                            .handle()
                            .insert_client(stream, Arc::new(ClientState::default()))
                            .unwrap();
                        compositor.clients.push(client);
                    }

                    compositor
                        .display
                        .dispatch_clients(&mut compositor.state)
                        .pb_expect("Failed to dispatch clients");
                    compositor
                        .display
                        .flush_clients()
                        .pb_expect("Failed to flush clients");
                }

                // It is important that all events on the display have been dispatched and flushed to clients before
                // swapping buffers because this operation may block.
                winit.submit(Some(&[damage])).unwrap();
            }

            // Redraw the application.
            //
            // It's preferable for applications that do not render continuously to render in
            // this event rather than in AboutToWait, since rendering in here allows
            // the program to gracefully handle redraws requested by the OS.

            // Draw.

            // Queue a RedrawRequested event.
            //
            // You only need to call this if you've determined that you need to redraw in
            // applications which do not always need to. Applications that redraw continuously
            // can render here instead.
            backend
                .graphic_renderer
                .as_ref()
                .unwrap()
                .window()
                .request_redraw();
        }
        CentralizedEvent::Input(event) => match event {
            InputEvent::Keyboard { event } => {
                let compositor = &mut backend.compositor;
                let state = &mut compositor.state;
                let serial = SERIAL_COUNTER.next_serial();
                let time = compositor.start_time.elapsed().as_millis() as u32;
                compositor.keyboard.input::<(), _>(
                    state,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |_, _, _| {
                        //
                        FilterResult::Forward
                    },
                );
            }
            InputEvent::TouchDown { event } => {
                let compositor = &mut backend.compositor;
                let state = &mut compositor.state;
                if let Some(surface) = get_surface(state) {
                    compositor.keyboard.set_focus(
                        state,
                        Some(surface.wl_surface().clone()),
                        0.into(),
                    );
                    let serial = SERIAL_COUNTER.next_serial();
                    let time = compositor.start_time.elapsed().as_millis() as u32;
                    compositor.touch.down(
                        state,
                        Some((surface.wl_surface().clone(), (0f64, 0f64).into())),
                        &touch::DownEvent {
                            slot: event.slot(),
                            location: (event.x(), event.y()).into(),
                            serial,
                            time,
                        },
                    );
                };
            }
            InputEvent::TouchUp { event } => {
                let compositor = &mut backend.compositor;
                let state = &mut compositor.state;
                if let Some(_surface) = get_surface(state) {
                    let serial = SERIAL_COUNTER.next_serial();
                    let time = compositor.start_time.elapsed().as_millis() as u32;
                    compositor.touch.up(
                        state,
                        &touch::UpEvent {
                            slot: event.slot(),
                            serial,
                            time,
                        },
                    );
                };
            }
            InputEvent::TouchMotion { event } => {
                let compositor = &mut backend.compositor;
                let state = &mut compositor.state;
                if let Some(surface) = get_surface(state) {
                    let time = compositor.start_time.elapsed().as_millis() as u32;
                    compositor.touch.motion(
                        state,
                        Some((surface.wl_surface().clone(), (0f64, 0f64).into())),
                        &touch::MotionEvent {
                            slot: event.slot(),
                            location: (event.x(), event.y()).into(),
                            time,
                        },
                    );
                };
            }
            // InputEvent::PointerMotion { event } => {
            //     let compositor = &mut backend.compositor;
            //     let pointer = compositor.pointer.clone();

            //     let mut pointer_location = pointer.current_location();
            //     let serial = SERIAL_COUNTER.next_serial();

            //     let pointer = pointer.clone();

            //     let mut pointer_locked = false;
            //     let mut pointer_confined = false;
            //     let mut confine_region = None;

            //     if let Some(surface) = get_surface(&compositor.state) {
            //         with_pointer_constraint(surface.wl_surface(), &pointer, |constraint| {
            //             match constraint {
            //                 Some(constraint) if constraint.is_active() => {
            //                     // Constraint does not apply if not within region
            //                     if !constraint
            //                         .region()
            //                         .map_or(true, |x| x.contains((pointer_location).to_i32_round()))
            //                     {
            //                         return;
            //                     }
            //                     match &*constraint {
            //                         PointerConstraint::Locked(_locked) => {
            //                             pointer_locked = true;
            //                         }
            //                         PointerConstraint::Confined(confine) => {
            //                             pointer_confined = true;
            //                             confine_region = confine.region().cloned();
            //                         }
            //                     }
            //                 }
            //                 _ => {}
            //             }
            //         });

            //         pointer.relative_motion(
            //             &mut compositor.state,
            //             Some((surface.wl_surface().clone(), pointer_location)),
            //             &pointer::RelativeMotionEvent {
            //                 delta: event.delta(),
            //                 delta_unaccel: event.delta_unaccel(),
            //                 utime: event.time(),
            //             },
            //         );

            //         // If pointer is locked, only emit relative motion
            //         if pointer_locked {
            //             pointer.frame(&mut compositor.state);
            //             return;
            //         }

            //         pointer_location += event.delta();

            //         // clamp to screen limits
            //         // this event is never generated by winit
            //         pointer_location = clamp_coords(compositor.state.space, pointer_location);

            //         // If confined, don't move pointer if it would go outside surface or region
            //         if pointer_confined {
            //             if let Some(region) = confine_region {
            //                 if !region.contains((pointer_location).to_i32_round()) {
            //                     pointer.frame(&mut compositor.state);
            //                     return;
            //                 }
            //             }
            //         }

            //         pointer.motion(
            //             &mut compositor.state,
            //             Some((surface.wl_surface().clone(), pointer_location)),
            //             &pointer::MotionEvent {
            //                 location: pointer_location,
            //                 serial,
            //                 time: event.time_msec(),
            //             },
            //         );
            //         pointer.frame(&mut compositor.state);

            //         with_pointer_constraint(&surface.wl_surface(), &pointer, |constraint| {
            //             match constraint {
            //                 Some(constraint) if !constraint.is_active() => {
            //                     let point = (pointer_location).to_i32_round();
            //                     if constraint
            //                         .region()
            //                         .map_or(true, |region| region.contains(point))
            //                     {
            //                         constraint.activate();
            //                     }
            //                 }
            //                 _ => {}
            //             }
            //         });
            //     }
            // }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let compositor = &mut backend.compositor;
                let pointer = compositor.pointer.clone();
                let space = &compositor.state.space;
                let serial = SERIAL_COUNTER.next_serial();

                let max_x = space
                    .outputs()
                    .fold(0, |acc, o| acc + space.output_geometry(o).unwrap().size.w);

                let max_h_output = space
                    .outputs()
                    .max_by_key(|o| space.output_geometry(o).unwrap().size.h)
                    .unwrap();

                let max_y = space.output_geometry(max_h_output).unwrap().size.h;

                let mut pointer_location =
                    (event.x_transformed(max_x), event.y_transformed(max_y)).into();

                // clamp to screen limits
                pointer_location = clamp_coords(space, pointer_location);

                if let Some(surface) = get_surface(&compositor.state) {
                    pointer.motion(
                        &mut compositor.state,
                        Some((surface.wl_surface().clone(), pointer_location)),
                        &pointer::MotionEvent {
                            location: pointer_location,
                            serial,
                            time: event.time_msec(),
                        },
                    );
                }
                pointer.frame(&mut compositor.state);
            }
            InputEvent::PointerButton { event, .. } => {
                let serial = SERIAL_COUNTER.next_serial();
                let button = event.button_code();

                let state = ButtonState::from(event.state());

                let compositor = &mut backend.compositor;
                let pointer = compositor.pointer.clone();

                // if ButtonState::Pressed == state {
                //     compositor
                //         .state
                //         .update_keyboard_focus(pointer.current_location(), serial);
                // };

                pointer.button(
                    &mut compositor.state,
                    &pointer::ButtonEvent {
                        button,
                        state: state.try_into().unwrap(),
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(&mut compositor.state);
            }
            InputEvent::PointerAxis { event } => {
                let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.
                });
                let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.
                });
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                {
                    let mut frame =
                        pointer::AxisFrame::new(event.time_msec()).source(event.source());
                    if horizontal_amount != 0.0 {
                        frame = frame.relative_direction(
                            Axis::Horizontal,
                            event.relative_direction(Axis::Horizontal),
                        );
                        frame = frame.value(Axis::Horizontal, horizontal_amount);
                        if let Some(discrete) = horizontal_amount_discrete {
                            frame = frame.v120(Axis::Horizontal, discrete as i32);
                        }
                    }
                    if vertical_amount != 0.0 {
                        frame = frame.relative_direction(
                            Axis::Vertical,
                            event.relative_direction(Axis::Vertical),
                        );
                        frame = frame.value(Axis::Vertical, vertical_amount);
                        if let Some(discrete) = vertical_amount_discrete {
                            frame = frame.v120(Axis::Vertical, discrete as i32);
                        }
                    }
                    if event.source() == AxisSource::Finger {
                        if event.amount(Axis::Horizontal) == Some(0.0) {
                            frame = frame.stop(Axis::Horizontal);
                        }
                        if event.amount(Axis::Vertical) == Some(0.0) {
                            frame = frame.stop(Axis::Vertical);
                        }
                    }
                    let compositor = &mut backend.compositor;
                    let pointer = compositor.pointer.clone();
                    pointer.axis(&mut compositor.state, frame);
                    pointer.frame(&mut compositor.state);
                }
            }
            _ => {}
        },
        _ => (),
    }
}
