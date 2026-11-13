import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings and accounts, as a modal sheet over the window.
Rectangle {
    id: s
    required property var be
    required property var prefs
    required property var accounts
    required property string lang
    signal closeRequested()
    signal changed()
    color: "#59000000"
    property string tab: "general"
    property string testMsg: ""
    property bool testOk: false
    property bool adding: false
    property bool googleBusy: false

    function parse(j) { try { return JSON.parse(j) } catch (e) { return null } }
    function setPref(k, v) { be.set_setting(k, JSON.stringify(v)); s.changed() }

    Connections {
        target: s.be
        function onGoogle_done(ok, message) {
            s.googleBusy = false
            s.testOk = ok
            s.testMsg = ok ? qsTr("Signed in as %1").arg(message) : message
            if (ok) { s.adding = false; s.changed(); s.be.sync_now() }
        }
        function onAccount_tested(ok, message) { s.testOk = ok; s.testMsg = ok ? qsTr("Connected — %1 calendar(s) found").arg(message) : message }
    }

    MouseArea { anchors.fill: parent; onClicked: s.closeRequested() }
    Keys.onEscapePressed: s.closeRequested()
    focus: true

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(760, parent.width - 40); height: Math.min(560, parent.height - 40)
        radius: 12; color: Theme.bg; border.color: Theme.line; clip: true
        MouseArea { anchors.fill: parent }
        Row {
            anchors.fill: parent
            Rectangle {
                width: 170; height: parent.height; color: Theme.panel
                Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: Theme.line }
                Column {
                    x: 10; y: 14; width: parent.width - 20; spacing: 2
                    Text { text: qsTr("Settings"); color: Theme.fg; font.pixelSize: 15; font.weight: Font.DemiBold; bottomPadding: 10; leftPadding: 6 }
                    Repeater {
                        model: [{ k: "general", t: qsTr("General") }, { k: "accounts", t: qsTr("Accounts") }, { k: "about", t: qsTr("About") }]
                        delegate: Rectangle {
                            required property var modelData
                            width: parent.width; height: 30; radius: 6
                            color: s.tab === modelData.k ? Theme.chip : "transparent"
                            Text { x: 8; anchors.verticalCenter: parent.verticalCenter; text: modelData.t; color: Theme.fg; font.pixelSize: 13; font.weight: s.tab === modelData.k ? Font.DemiBold : Font.Normal }
                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: s.tab = modelData.k }
                        }
                    }
                }
            }
            Item {
                width: parent.width - 170; height: parent.height
                Btn { anchors.right: parent.right; anchors.top: parent.top; anchors.margins: 12; text: "✕"; pad: 8; onClicked: s.closeRequested() }
                Flickable {
                    anchors.fill: parent; anchors.margins: 24; anchors.topMargin: 28
                    contentHeight: pageCol.implicitHeight
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar { width: 8 }
                    Column {
                        id: pageCol
                        width: parent.width - 12
                        spacing: 0

                        // ---------------- general
                        Column {
                            visible: s.tab === "general"
                            width: parent.width
                            spacing: 0
                            component Row2: Rectangle {
                                id: r2
                                property string label: ""
                                property string hint: ""
                                default property alias content: holder.data
                                width: pageCol.width; height: 44; color: "transparent"
                                Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                Column { anchors.verticalCenter: parent.verticalCenter; Text { text: r2.label; color: Theme.fg; font.pixelSize: 13 } Text { visible: r2.hint !== ""; text: r2.hint; color: Theme.mut; font.pixelSize: 11 } }
                                Row { id: holder; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; spacing: 6 }
                            }
                            Row2 {
                                label: qsTr("Language"); hint: qsTr("applied after restart")
                                Repeater { model: [{ k: "auto", t: qsTr("Auto") }, { k: "en", t: "English" }, { k: "pl", t: "Polski" }]
                                    delegate: Btn { required property var modelData; text: modelData.t; on: s.prefs.language === modelData.k; onClicked: s.setPref("language", modelData.k) } }
                            }
                            Row2 {
                                label: qsTr("Theme")
                                Repeater { model: [{ k: "system", t: qsTr("System") }, { k: "light", t: qsTr("Light") }, { k: "dark", t: qsTr("Dark") }, { k: "navy", t: qsTr("Navy") }]
                                    delegate: Btn { required property var modelData; text: modelData.t; on: (s.prefs.theme || "system") === modelData.k; onClicked: s.setPref("theme", modelData.k) } }
                            }
                            Row2 {
                                label: qsTr("Week starts on")
                                Repeater { model: [{ k: "monday", t: qsTr("Monday") }, { k: "sunday", t: qsTr("Sunday") }, { k: "saturday", t: qsTr("Saturday") }]
                                    delegate: Btn { required property var modelData; text: modelData.t; on: s.prefs.weekStart === modelData.k; onClicked: s.setPref("week_start", modelData.k) } }
                            }
                            Row2 {
                                label: qsTr("Time format")
                                Repeater { model: [{ k: "24h", t: "24 h" }, { k: "12h", t: "12 h" }, { k: "system", t: qsTr("System") }]
                                    delegate: Btn { required property var modelData; text: modelData.t; on: s.prefs.timeFormat === modelData.k; onClicked: s.setPref("time_format", modelData.k) } }
                            }
                            Row2 {
                                label: qsTr("Default view")
                                Repeater { model: [{ k: "day", t: qsTr("Day") }, { k: "week", t: qsTr("Week") }, { k: "month", t: qsTr("Month") }]
                                    delegate: Btn { required property var modelData; text: modelData.t; on: s.prefs.defaultView === modelData.k; onClicked: s.setPref("default_view", modelData.k) } }
                            }
                            Row2 {
                                label: qsTr("Show weekends")
                                Btn { text: s.prefs.showWeekends === false ? qsTr("No") : qsTr("Yes"); on: true; onClicked: s.setPref("show_weekends", s.prefs.showWeekends === false) }
                            }
                            Row2 {
                                label: qsTr("Default reminder"); hint: qsTr("minutes before the start; −1 = none")
                                Btn { text: "−"; pad: 9; onClicked: s.setPref("default_reminder", Math.max(-1, (s.prefs.defaultReminder === undefined ? 10 : s.prefs.defaultReminder) - 5)) }
                                Text { text: s.prefs.defaultReminder; color: Theme.fg; font.pixelSize: 13; width: 34; horizontalAlignment: Text.AlignHCenter; anchors.verticalCenter: parent.verticalCenter }
                                Btn { text: "+"; pad: 9; onClicked: s.setPref("default_reminder", Math.min(1440, (s.prefs.defaultReminder === undefined ? 10 : s.prefs.defaultReminder) + 5)) }
                            }
                            Row2 {
                                label: qsTr("Desktop notifications")
                                Btn { text: s.prefs.notifications === false ? qsTr("Off") : qsTr("On"); on: true; onClicked: s.setPref("notifications", s.prefs.notifications === false) }
                            }
                            Row2 {
                                label: qsTr("Sync every"); hint: qsTr("minutes")
                                Btn { text: "−"; pad: 9; onClicked: s.setPref("sync_interval", Math.max(1, s.prefs.syncInterval - 5)) }
                                Text { text: s.prefs.syncInterval; color: Theme.fg; font.pixelSize: 13; width: 34; horizontalAlignment: Text.AlignHCenter; anchors.verticalCenter: parent.verticalCenter }
                                Btn { text: "+"; pad: 9; onClicked: s.setPref("sync_interval", Math.min(240, s.prefs.syncInterval + 5)) }
                            }
                        }

                        // ---------------- accounts
                        Column {
                            visible: s.tab === "accounts"
                            width: parent.width
                            spacing: 8
                            Text { text: qsTr("Accounts and calendars"); color: Theme.fg; font.pixelSize: 15; font.weight: Font.DemiBold }
                            Repeater {
                                model: s.accounts
                                delegate: Rectangle {
                                    required property var modelData
                                    width: pageCol.width; height: 52; radius: 8; color: Theme.bg2; border.color: Theme.line
                                    Column {
                                        x: 12; anchors.verticalCenter: parent.verticalCenter
                                        Text { text: modelData.name; color: Theme.fg; font.pixelSize: 13; font.weight: Font.DemiBold }
                                        Text { text: (modelData.kind === "local" ? qsTr("on this computer") : (modelData.kind === "ics" ? qsTr("ICS subscription") : (modelData.kind === "google" ? "Google Calendar · " + modelData.username : "CalDAV"))) + " · " + qsTr("%n calendar(s)", "", modelData.calendars) + (modelData.status ? " · " + modelData.status : ""); color: Theme.mut; font.pixelSize: 11 }
                                    }
                                    Btn { visible: modelData.kind !== "local"; anchors.right: parent.right; anchors.rightMargin: 10; anchors.verticalCenter: parent.verticalCenter; text: modelData.kind === "google" ? qsTr("Disconnect") : qsTr("Remove"); onClicked: { s.be.remove_account(modelData.key); s.changed() } }
                                }
                            }
                            Row {
                                spacing: 6
                                Btn { text: qsTr("Sync now"); onClicked: s.be.sync_now() }
                                Btn { text: s.adding ? qsTr("Cancel") : qsTr("Add account…"); primary: !s.adding; onClicked: { s.adding = !s.adding; s.testMsg = "" } }
                            }
                            Column {
                                visible: s.adding
                                width: parent.width
                                spacing: 6
                                property string kind: "caldav"
                                id: form
                                Row {
                                    spacing: 8
                                    Btn {
                                        text: s.googleBusy ? qsTr("Waiting for the browser… (click to cancel)") : qsTr("Sign in with Google"); primary: true
                                        onClicked: {
                                            if (s.googleBusy) { s.be.google_cancel(); s.googleBusy = false; s.testMsg = ""; return }
                                            s.testOk = false
                                            if (!s.be.google_configured()) { s.testMsg = qsTr("Google sign-in needs your own OAuth client: set client_id and client_secret in the [google] section of config.toml (see the README)."); return }
                                            s.googleBusy = true; s.testMsg = qsTr("Complete the sign-in in your browser…"); s.be.google_login()
                                        }
                                    }
                                    Text { text: qsTr("or add by address:"); color: Theme.mut; font.pixelSize: 12; anchors.verticalCenter: parent.verticalCenter }
                                }
                                Row {
                                    spacing: 6
                                    Btn { text: "CalDAV"; on: form.kind === "caldav"; onClicked: form.kind = "caldav" }
                                    Btn { text: qsTr("ICS subscription"); on: form.kind === "ics"; onClicked: form.kind = "ics" }
                                }
                                Field { id: fName; width: parent.width; placeholderText: qsTr("Name (e.g. Work)") }
                                Field { id: fUrl; width: parent.width; placeholderText: form.kind === "caldav" ? qsTr("Server address (https://…)") : qsTr("Link to the .ics file (secret address in iCal format)") }
                                Field { id: fUser; visible: form.kind === "caldav"; width: parent.width; placeholderText: qsTr("User name / e-mail") }
                                Field { id: fPass; visible: form.kind === "caldav"; width: parent.width; echoMode: TextInput.Password; placeholderText: qsTr("Password (kept in the system keyring)") }
                                Row {
                                    spacing: 6
                                    Btn { text: qsTr("Test connection"); onClicked: { s.testMsg = qsTr("Checking…"); s.testOk = false; s.be.test_account(form.kind, fUrl.text, fUser.text, fPass.text) } }
                                    Btn {
                                        text: qsTr("Add"); primary: true
                                        onClicked: {
                                            const r = s.parse(s.be.add_account(form.kind, fName.text, fUrl.text, fUser.text, fPass.text, ""))
                                            if (r && r.error) { s.testOk = false; s.testMsg = r.error; return }
                                            fName.text = ""; fUrl.text = ""; fUser.text = ""; fPass.text = ""; s.adding = false; s.testMsg = ""
                                            s.changed(); s.be.sync_now()
                                        }
                                    }
                                }
                                Text { visible: s.testMsg !== ""; width: parent.width; wrapMode: Text.Wrap; text: s.testMsg; color: s.testOk ? "#0f9d58" : "#e16259"; font.pixelSize: 12 }
                            }
                        }

                        // ---------------- about
                        Column {
                            visible: s.tab === "about"
                            width: parent.width
                            spacing: 8
                            Image { source: "qrc:/qt/qml/org/kronoa/icons/kronoa-64.png"; width: 64; height: 64; sourceSize.width: 128; sourceSize.height: 128; mipmap: true }
                            Text { text: "Kronoa " + s.be.version(); color: Theme.fg; font.pixelSize: 17; font.weight: Font.DemiBold }
                            Text { width: parent.width; wrapMode: Text.Wrap; text: qsTr("A fast, native calendar: Rust engine, Qt 6 interface. Local calendars, CalDAV accounts and ICS subscriptions. MIT or Apache-2.0."); color: Theme.mut; font.pixelSize: 13 }
                        }
                    }
                }
            }
        }
    }
}
