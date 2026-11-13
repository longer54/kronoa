//! Kronoa: window, application object and QML engine. Everything else lives in kronoa-core.

mod bridge;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};

const APP_NAME: &str = "Kronoa";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("kronoa {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Kronoa {}\n\nUsage: kronoa [--profile=NAME] [--demo] [--autotest=DIR] [--version]\n\n  --profile=NAME   separate config and database (default: default)\n  --demo           sample events in an empty profile (for screenshots)\n  --autotest=DIR   offline scripted run: saves screenshots into DIR, then quits\n\nEnvironment: KRONOA_LANG=auto|en|pl overrides the language setting.",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    let profile = args.iter().find_map(|a| a.strip_prefix("--profile=")).unwrap_or("default").to_string();
    let demo = args.iter().any(|a| a == "--demo" || a.starts_with("--autotest"));
    bridge::init(&profile, demo);
    if std::env::var_os("KRONOA_LANG").is_none() {
        std::env::set_var("KRONOA_LANG", bridge::language());
    }
    let mut app = bridge::kronoa_new_application(&args);
    if let Some(mut a) = app.as_mut() {
        a.as_mut().set_application_name(&QString::from(APP_NAME));
        a.as_mut().set_application_display_name(&QString::from(APP_NAME));
        QGuiApplication::set_desktop_file_name(&QString::from("kronoa"));
    }
    eprintln!("kronoa: starting QML");
    let mut qml = QQmlApplicationEngine::new();
    if let Some(qml) = qml.as_mut() {
        qml.load(&QUrl::from("qrc:/qt/qml/org/kronoa/qml/Main.qml"));
    }
    eprintln!("kronoa: entering event loop");
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
