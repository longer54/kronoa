#pragma once

#include <memory>

#include <QtGui/QGuiApplication>

#include "rust/cxx.h"

// A QApplication (Qt Widgets) behind the QGuiApplication API: the tray menu and file dialogs of some
// desktop platform themes (KDE) are widgets. Also installs the translator and the window icon.
std::unique_ptr<QGuiApplication> kronoa_new_application(rust::Slice<const rust::String> args);
