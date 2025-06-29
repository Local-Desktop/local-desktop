use super::build::{PolarBearApp, PolarBearBackend};
use crate::app::event_centralizer::{centralize_device_event, centralize_window_event};
use crate::app::event_handler::handle;
use crate::proot::launch::launch;
use crate::utils::config;
use crate::utils::ndk::run_in_jvm;
use crate::utils::webview::show_webview_popup;
use crate::wayland::compositor::State;
use crate::wayland::winit_backend::bind;
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::utils::Transform;
use std::thread;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

impl ApplicationHandler for PolarBearApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match self.backend {
            PolarBearBackend::WebView(ref mut backend) => {
                let port = backend.socket_port;
                let url = format!("file:///android_asset/setup-progress.html?port={}", port);
                run_in_jvm(
                    move |env, app| {
                        show_webview_popup(env, app, &url);
                    },
                    self.frontend.android_app.clone(),
                );
            }
            PolarBearBackend::Wayland(ref mut backend) => {
                // Initialize the Wayland backend
                let winit = bind(&event_loop);
                let window_size = winit.window_size();
                let scale_factor = winit.scale_factor();
                let size = (window_size.w, window_size.h);
                backend.graphic_renderer = Some(winit);
                backend.compositor.state.size = size.into();

                // Create the Output with given name and physical properties.
                let output = Output::new(
                    "Local Desktop Wayland Compositor".into(), // the name of this output,
                    PhysicalProperties {
                        size: size.into(),                 // dimensions (width, height) in mm
                        subpixel: Subpixel::HorizontalRgb, // subpixel information
                        make: "Local Desktop".into(),      // make of the monitor
                        model: config::VERSION.into(),     // model of the monitor
                    },
                );

                let dh = backend.compositor.display.handle();
                // create a global, if you want to advertise it to clients
                let _global = output.create_global::<State>(
                    &dh, // the display
                ); // you can drop the global, if you never intend to destroy it.
                   // Now you can configure it
                output.change_current_state(
                    Some(Mode {
                        size: size.into(),
                        refresh: 60000,
                    }), // the resolution mode,
                    Some(Transform::Normal), // global screen transformation
                    Some(Scale::Fractional(scale_factor)), // global screen scaling factor
                    Some((0, 0).into()),     // output position
                );
                // set the preferred mode
                output.set_preferred(Mode {
                    size: size.into(),
                    refresh: 60000,
                });

                thread::spawn(move || {
                    let launch_command = "dbus-launch startxfce4".to_string();
                    launch(launch_command);
                });
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let PolarBearBackend::Wayland(backend) = &mut self.backend {
            // Map raw events to our own events
            let event = centralize_window_event(event, backend);

            // Handle the centralized events
            handle(event, backend, event_loop);
        }
    }

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if let PolarBearBackend::Wayland(backend) = &mut self.backend {
            // Map raw events to our own events
            let event = centralize_device_event(event, backend);

            // Handle the centralized events
            handle(event, backend, event_loop);
        }
    }
}
