mod commands;
pub mod db;
pub mod models;
pub mod http_client;
pub mod services;

fn main() {
    commands::run_app();
}
