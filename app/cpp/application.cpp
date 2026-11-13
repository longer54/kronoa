#include "application.h"

#include <string>
#include <vector>

#include <QtWidgets/QApplication>
#include <QtCore/QTranslator>
#include <QtCore/QDir>
#include <QtCore/QCoreApplication>
#include <QtCore/QLocale>
#include <QtGui/QIcon>

std::unique_ptr<QGuiApplication> kronoa_new_application(rust::Slice<const rust::String> args)
{
    static std::vector<std::string> storage;
    static std::vector<char*> argv;
    static int argc = 0;
    storage.clear();
    for (const auto& a : args)
        storage.emplace_back(std::string(a));
    argv.clear();
    for (auto& s : storage)
        argv.push_back(s.data());
    argv.push_back(nullptr);
    argc = static_cast<int>(storage.size());
    auto app = std::make_unique<QApplication>(argc, argv.data());

    // window icon: the owner's logo, built into the program
    QIcon icon;
    for (int s : { 16, 24, 32, 48, 64, 128, 256 })
        icon.addFile(QStringLiteral(":/qt/qml/org/kronoa/icons/kronoa-%1.png").arg(s), QSize(s, s));
    app->setWindowIcon(icon);

    // interface language: KRONOA_LANG = auto | en | pl (set by main.rs from the `language` setting)
    QString lang = qEnvironmentVariable("KRONOA_LANG").trimmed().toLower();
    if (lang.isEmpty() || lang == QStringLiteral("auto"))
        lang = QLocale::system().name().startsWith(QStringLiteral("pl")) ? QStringLiteral("pl") : QStringLiteral("en");
    if (lang != QStringLiteral("en")) {
        static QTranslator tr;
        const QString name = QStringLiteral("kronoa_") + lang + QStringLiteral(".qm");
        const QString dataHome = qEnvironmentVariable("XDG_DATA_HOME", QDir::homePath() + QStringLiteral("/.local/share"));
        const QStringList dirs = { QCoreApplication::applicationDirPath(),
                                   dataHome + QStringLiteral("/kronoa"),
                                   QCoreApplication::applicationDirPath() + QStringLiteral("/../share/kronoa/translations"),
                                   QStringLiteral("/usr/share/kronoa/translations"),
                                   QStringLiteral(":/qt/qml/org/kronoa/i18n") };
        for (const auto& d : dirs) {
            if (tr.load(name, d)) { QCoreApplication::installTranslator(&tr); break; }
        }
    }
    return app;
}
