mod commands;
pub mod db;
pub mod http_client;
pub mod models;
pub mod services;

fn main() {
    commands::run_app();
}
