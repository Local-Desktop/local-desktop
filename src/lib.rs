#![cfg(target_os = "android")]

pub mod android_main;
pub mod app {
    pub mod build;
    pub mod event_centralizer;
    pub mod event_handler;
    pub mod run;
}
pub mod wayland {
    pub mod compositor;
    pub mod element;
    pub mod input;
    pub mod keymap;
    pub mod winit_backend;
}
pub mod proot {
    pub mod launch;
    pub mod process;
    pub mod setup;
}
pub mod utils {
    pub mod application_context;
    pub mod config;
    pub mod fullscreen_immersive;
    pub mod logging;
    pub mod ndk;
    pub mod socket;
    pub mod webview;
}
