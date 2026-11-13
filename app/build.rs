use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    let mut res: Vec<String> = [16, 24, 32, 48, 64, 128, 256].iter().map(|s| format!("icons/kronoa-{s}.png")).collect();
    // compiled translation (lrelease6 i18n/kronoa_pl.ts), if present: the fallback when no .qm is installed
    let qm = "i18n/kronoa_pl.qm";
    println!("cargo:rerun-if-changed={qm}");
    if std::path::Path::new(qm).exists() {
        res.push(qm.into());
    }
    CxxQtBuilder::new_qml_module(QmlModule::new("org.kronoa").qml_files([
        QmlFile::from("qml/Theme.qml").singleton(true),
        "qml/Main.qml".into(),
        "qml/Settings.qml".into(),
        "qml/Btn.qml".into(),
        "qml/Field.qml".into(),
    ]))
    .qrc_resources(res)
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qt_module("Widgets")
    .files(["src/bridge.rs"])
    .cpp_file("cpp/application.cpp")
    .include_dir("cpp")
    .build();
}
