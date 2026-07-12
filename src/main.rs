#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod dashboard;
mod integrations;
mod model;
mod persistence;
mod recurring;
mod timer;
mod wheel;

fn main() -> eframe::Result {
    app::run()
}
