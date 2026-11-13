import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt.labs.platform as Platform
import org.kronoa

ApplicationWindow {
    id: win
    width: 1280
    height: 820
    minimumWidth: 760
    minimumHeight: 480
    visible: true
    title: "Kronoa"
    color: Theme.bg
    flags: Qt.Window | Qt.FramelessWindowHint
    font.family: Theme.font
    font.pixelSize: 13

    palette.window: Theme.bg
    palette.windowText: Theme.fg
    palette.base: Theme.bg
    palette.text: Theme.fg
    palette.button: Theme.chip
    palette.buttonText: Theme.fg
    palette.highlight: Theme.sel
    palette.highlightedText: Theme.onAcc
    palette.mid: Theme.line
    palette.placeholderText: Theme.mut

    Backend { id: backend }
    Component.onDestruction: console.warn("main destroyed")

    // ---- state ------------------------------------------------------------------------------
    readonly property real hh: 56                  // pixels per hour
    readonly property real gutter: 56              // hour labels
    readonly property real rightPad: 8
    property var prefs: ({})
    property var cals: []
    property var occs: []
    property var dayList: []
    property var placed: []
    property var allDayByDay: []
    property var monthCells: []
    property var upcoming: []
    property var todayList: []
    property var accounts: []
    property string view: "week"
    property date anchor: new Date()
    property int selId: -1
    property real selOcc: 0
    property int draftCol: -1
    property var detail: null
    property bool sideOpen: true
    property real nowMs: Date.now()
    property string toastMsg: ""
    property bool toastUndo: false
    property bool paletteOpen: false
    property bool settingsOpen: false
    property string lang: "en"
    property var loc: Qt.locale("en_US")
    readonly property bool isPl: lang === "pl"
    readonly property bool compactRight: width < 1000

    readonly property var autotestArg: {
        const a = Qt.application.arguments.find(x => x.startsWith("--autotest="))
        return a ? a.substring(11) : ""
    }

    // ---- date helpers -----------------------------------------------------------------------
    function sod(d) { return new Date(d.getFullYear(), d.getMonth(), d.getDate()) }
    function addDays(d, n) { return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n) }
    function pad2(n) { return (n < 10 ? "0" : "") + n }
    function ymd(d) { return d.getFullYear() + "-" + pad2(d.getMonth() + 1) + "-" + pad2(d.getDate()) }
    function sameDay(a, b) { return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate() }
    function applyTheme() {
        if (autotestArg !== "" && autoStep > 0) return
        const arg = Qt.application.arguments.find(a => a.startsWith("--theme="))
        let t = arg ? arg.substring(8) : (prefs.theme || "system")
        if (t === "system") t = Qt.styleHints.colorScheme === Qt.ColorScheme.Dark ? "navy" : "light"
        Theme.name = (t === "dark" || t === "navy") ? t : "light"
    }
    function firstDow() { const w = prefs.weekStart; return w === "sunday" ? 0 : (w === "saturday" ? 6 : 1) }
    function weekStart(d) { const back = (d.getDay() - firstDow() + 7) % 7; return addDays(sod(d), -back) }
    function cap(s) { return s.length ? s.charAt(0).toUpperCase() + s.slice(1) : s }
    function monthName(m) { return cap(loc.standaloneMonthName(m, Locale.LongFormat)) }
    function monthGen(m) { return loc.monthName(m, Locale.LongFormat) }
    function dowShort(jsDay) {
        const pl = ["Nd", "Pn", "Wt", "Śr", "Cz", "Pt", "So"], en = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        return (isPl ? pl : en)[jsDay]
    }
    function dowLong(jsDay) { return cap(loc.standaloneDayName(jsDay, Locale.LongFormat)) }
    function fmtT(ms) {
        const d = new Date(ms)
        if (prefs.timeFormat === "12h") {
            const h = d.getHours()
            return (h % 12 === 0 ? 12 : h % 12) + ":" + pad2(d.getMinutes()) + (h < 12 ? " am" : " pm")
        }
        if (prefs.timeFormat === "system") return Qt.formatTime(d, loc.timeFormat(Locale.ShortFormat))
        return d.getHours() + ":" + pad2(d.getMinutes())
    }
    function fmtHM(ms) { const d = new Date(ms); return pad2(d.getHours()) + ":" + pad2(d.getMinutes()) }
    function parseHM(t) {
        const m = /^\s*(\d{1,2})(?:[:.](\d{2}))?\s*$/.exec(t)
        if (!m) return null
        const h = parseInt(m[1]), mi = m[2] ? parseInt(m[2]) : 0
        return (h > 23 || mi > 59) ? null : [h, mi]
    }
    function fmtHour(h) { return fmtT(new Date(2000, 0, 1, h, 0).getTime()) }
    function rangeText(o) { return o.allDay ? qsTr("all day") : fmtT(o.startMs) + "–" + fmtT(o.endMs) }
    function titleText() {
        if (view === "month") return monthName(anchor.getMonth()) + " " + anchor.getFullYear()
        if (view === "day") return dowLong(anchor.getDay()) + ", " + anchor.getDate() + " " + monthGen(anchor.getMonth())
        const a = dayList[0], b = dayList[dayList.length - 1]
        if (!a) return ""
        if (a.getMonth() === b.getMonth()) return a.getDate() + "–" + b.getDate() + " " + monthGen(b.getMonth()) + " " + b.getFullYear()
        return a.getDate() + " " + monthGen(a.getMonth()) + " – " + b.getDate() + " " + monthGen(b.getMonth()) + " " + b.getFullYear()
    }
    function parse(j) { try { const v = JSON.parse(j); return v } catch (e) { return null } }
    function repeatLabel(k) {
        return ({ none: qsTr("no"), daily: qsTr("daily"), weekdays: qsTr("on weekdays"), weekly: qsTr("weekly"), monthly: qsTr("monthly"), yearly: qsTr("yearly") })[k] || k
    }

    // ---- data -------------------------------------------------------------------------------
    function visibleDays() {
        if (view === "day") return [sod(anchor)]
        const s = weekStart(anchor), out = []
        for (let i = 0; i < 7; i++) {
            const d = addDays(s, i)
            if (prefs.showWeekends === false && (d.getDay() === 0 || d.getDay() === 6)) continue
            out.push(d)
        }
        return out
    }

    function layoutTimed(days, list) {
        const out = []
        for (let i = 0; i < days.length; i++) {
            const ds = days[i].getTime(), de = addDays(days[i], 1).getTime()
            const items = []
            for (const o of list) {
                if (o.allDay) continue
                if (!(o.endMs > ds && o.startMs < de) && !(o.startMs === o.endMs && o.startMs >= ds && o.startMs < de)) continue
                const top = (Math.max(o.startMs, ds) - ds) / 60000
                const bottom = Math.max((Math.min(o.endMs, de) - ds) / 60000, top + 20)
                items.push({ o: o, day: i, top: top, bottom: bottom, lane: 0, lanes: 1 })
            }
            items.sort((a, b) => a.top - b.top || b.bottom - a.bottom)
            let cluster = [], laneEnds = [], clusterEnd = -1
            const flush = () => { for (const it of cluster) it.lanes = Math.max(laneEnds.length, 1); cluster = []; laneEnds = []; clusterEnd = -1 }
            for (const it of items) {
                if (cluster.length && it.top >= clusterEnd) flush()
                let l = laneEnds.findIndex(e => e <= it.top)
                if (l < 0) { l = laneEnds.length; laneEnds.push(0) }
                laneEnds[l] = it.bottom
                it.lane = l
                cluster.push(it)
                clusterEnd = Math.max(clusterEnd, it.bottom)
            }
            flush()
            for (const it of items) out.push(it)
        }
        return out
    }

    function refresh() {
        prefs = parse(backend.settings_json()) || {}
        applyTheme()
        cals = parse(backend.calendars_json()) || []
        nowMs = Date.now()
        if (view === "month") {
            const first = weekStart(new Date(anchor.getFullYear(), anchor.getMonth(), 1))
            const from = first, to = addDays(first, 42)
            occs = parse(backend.occurrences_json(from.getTime(), to.getTime())) || []
            const cells = []
            for (let i = 0; i < 42; i++) {
                const d = addDays(first, i), ds = d.getTime(), de = addDays(d, 1).getTime(), key = ymd(d)
                const items = occs.filter(o => o.allDay ? (o.startDate <= key && key < o.endDate) : (o.endMs > ds && o.startMs < de) || o.startMs === ds)
                items.sort((a, b) => (b.allDay ? 1 : 0) - (a.allDay ? 1 : 0) || a.startMs - b.startMs)
                cells.push({ date: d, items: items, other: d.getMonth() !== anchor.getMonth() })
            }
            monthCells = cells
            dayList = []
            const hdr = []
            for (let i = 0; i < 7; i++) hdr.push(addDays(first, i))
            dayList = hdr
        } else {
            const days = visibleDays()
            dayList = days
            occs = parse(backend.occurrences_json(days[0].getTime(), addDays(days[days.length - 1], 1).getTime())) || []
            placed = layoutTimed(days, occs)
            allDayByDay = days.map(d => { const key = ymd(d); return occs.filter(o => o.allDay && o.startDate <= key && key < o.endDate) })
        }
        const t0 = Date.now(), t2 = addDays(new Date(), 2).getTime()
        upcoming = (parse(backend.occurrences_json(t0, t2)) || []).filter(o => !o.allDay)
        const td = sod(new Date())
        todayList = (parse(backend.occurrences_json(td.getTime(), addDays(td, 1).getTime())) || []).filter(o => !o.allDay)
        accounts = parse(backend.accounts_json()) || []
        if (selId >= 0) {
            detail = parse(backend.detail_json(selId, selOcc))
            if (!detail || detail.error) { selId = -1; detail = null }
        }
    }

    function select(id, occ) {
        draftCol = -1
        selId = id
        selOcc = occ
        detail = id >= 0 ? parse(backend.detail_json(id, occ)) : null
        if (detail && detail.error) { detail = null; selId = -1 }
    }

    function toast(msg, withUndo) {
        toastMsg = msg
        toastUndo = !!withUndo
        toastTimer.restart()
    }
    function doUndo() {
        const what = backend.undo()
        if (what !== "") { toastMsg = ""; select(-1, 0); refresh() }
    }
    function gotoToday() { anchor = new Date(); refresh(); scrollToHour(Math.max(0, (prefs.startHour || 7) - 0)) }
    function step(n) {
        if (view === "month") anchor = new Date(anchor.getFullYear(), anchor.getMonth() + n, 1)
        else if (view === "day") anchor = addDays(anchor, n)
        else anchor = addDays(anchor, 7 * n)
        refresh()
    }
    function setView(v) { draftCol = -1; view = v; refresh() }
    signal autoDraft(int col, string title, bool commit)
    function scrollToHour(h) { if (grid.contentHeight > 0) grid.contentY = Math.max(0, h * hh - 4) }
    function createFromText(text) {
        const r = parse(backend.create_text(text))
        if (!r || r.error) { toast(r ? r.error : qsTr("Could not add the event"), false); return false }
        anchor = new Date(r.start)
        paletteOpen = false
        refresh()
        select(r.id, r.start)
        toast(qsTr("Added “%1”").arg(r.title), true)
        return true
    }
    function quitApp() { Qt.quit() }
    function showWindow() { win.show(); win.raise(); win.requestActivate() }

    // ---- life cycle -------------------------------------------------------------------------
    Component.onCompleted: {
        lang = backend.lang()
        loc = Qt.locale(lang === "pl" ? "pl_PL" : "en_US")
        refresh()
        view = prefs.defaultView || "week"
        refresh()
        backend.sync_now()
        backend.check_reminders()
        if (autotestArg !== "") { win.width = 1280; win.height = 800; autoTimer.start() }
        else Qt.callLater(() => scrollToHour(prefs.startHour || 7))
    }
    Connections {
        target: backend
        function onSynced(summary) { refresh() }
    }
    Timer { id: syncTimer; interval: Math.max(1, prefs.syncInterval || 15) * 60000; running: true; repeat: true; onTriggered: backend.sync_now() }
    Timer { interval: 30000; running: true; repeat: true; onTriggered: { nowMs = Date.now(); backend.check_reminders(); } }
    Timer { id: toastTimer; interval: 4500; onTriggered: toastMsg = "" }

    // ---- tray -------------------------------------------------------------------------------
    Platform.SystemTrayIcon {
        id: tray
        visible: autotestArg === "" && available
        icon.source: "qrc:/qt/qml/org/kronoa/icons/kronoa-32.png"
        tooltip: "Kronoa"
        onActivated: (reason) => { if (reason === Platform.SystemTrayIcon.Context) return; if (win.visible && win.active) win.hide(); else win.showWindow() }
        menu: Platform.Menu {
            Platform.MenuItem { text: qsTr("Open Kronoa"); onTriggered: win.showWindow() }
            Platform.MenuItem { text: qsTr("Sync now"); onTriggered: backend.sync_now() }
            Platform.MenuItem { text: qsTr("Settings…"); onTriggered: { win.showWindow(); win.settingsOpen = true } }
            Platform.MenuSeparator {}
            Platform.MenuItem { text: qsTr("Quit"); onTriggered: win.quitApp() }
        }
    }

    // ---- shortcuts --------------------------------------------------------------------------
    Shortcut { sequence: "Ctrl+K"; onActivated: { palette.open() } }
    Shortcut { sequence: "Ctrl+\\"; onActivated: win.sideOpen = !win.sideOpen }
    Shortcut { sequence: "Ctrl+,"; onActivated: win.settingsOpen = true }
    Shortcut { sequence: "Ctrl+Z"; onActivated: win.doUndo() }
    Shortcut { sequence: "Ctrl+Q"; onActivated: win.quitApp() }

    FocusScope {
        id: root
        anchors.fill: parent
        focus: true
        Rectangle { anchors.fill: parent; color: Theme.bg; z: -1 }
        Keys.onPressed: (e) => {
            if (e.modifiers & (Qt.ControlModifier | Qt.AltModifier)) return
            if (e.key === Qt.Key_T) { win.gotoToday(); e.accepted = true }
            else if (e.key === Qt.Key_D) { win.setView("day"); e.accepted = true }
            else if (e.key === Qt.Key_W) { win.setView("week"); e.accepted = true }
            else if (e.key === Qt.Key_M) { win.setView("month"); e.accepted = true }
            else if (e.key === Qt.Key_Left) { win.step(-1); e.accepted = true }
            else if (e.key === Qt.Key_Right) { win.step(1); e.accepted = true }
            else if (e.key === Qt.Key_Escape) { win.select(-1, 0); e.accepted = true }
            else if ((e.key === Qt.Key_Delete || e.key === Qt.Key_Backspace) && win.selId >= 0) { win.removeSelected(); e.accepted = true }
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // ---- top bar -----------------------------------------------------------------
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 38
                color: Theme.bg
                Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                DragHandler { target: null; onActiveChanged: if (active) win.startSystemMove() }
                TapHandler { acceptedButtons: Qt.LeftButton; gesturePolicy: TapHandler.ReleaseWithinBounds; onDoubleTapped: win.visibility === Window.Maximized ? win.showNormal() : win.showMaximized() }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    spacing: 8
                    Image { source: "qrc:/qt/qml/org/kronoa/icons/kronoa-32.png"; sourceSize.width: 40; sourceSize.height: 40; Layout.preferredWidth: 22; Layout.preferredHeight: 22; smooth: true; mipmap: true }
                    Text { text: "Kronoa"; color: Theme.fg; font.weight: Font.Bold; font.pixelSize: 14; font.letterSpacing: 0.3 }
                    Item { Layout.fillWidth: true }
                    Repeater {
                        model: [{ k: "min", tip: qsTr("minimize") }, { k: "max", tip: qsTr("maximize") }, { k: "close", tip: qsTr("close") }]
                        delegate: Rectangle {
                            required property var modelData
                            Layout.preferredWidth: 12
                            Layout.preferredHeight: 12
                            Layout.leftMargin: 4
                            radius: 6
                            color: wma.containsMouse ? Theme.acc : Theme.chip
                            MouseArea {
                                id: wma
                                anchors.fill: parent
                                anchors.margins: -4
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                ToolTip.visible: containsMouse
                                ToolTip.text: modelData.tip
                                ToolTip.delay: 600
                                onClicked: {
                                    if (modelData.k === "min") win.showMinimized()
                                    else if (modelData.k === "max") win.visibility === Window.Maximized ? win.showNormal() : win.showMaximized()
                                    else if (tray.available && tray.visible) win.hide()
                                    else win.quitApp()
                                }
                            }
                        }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                // ---- left panel ----------------------------------------------------------
                Rectangle {
                    id: side
                    Layout.fillHeight: true
                    Layout.preferredWidth: win.sideOpen ? 240 : 0
                    visible: Layout.preferredWidth > 0 || sideAnim.running
                    clip: true
                    color: Theme.panel
                    Behavior on Layout.preferredWidth { NumberAnimation { id: sideAnim; duration: 180; easing.type: Easing.OutCubic } }
                    Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: Theme.line }
                    Flickable {
                        anchors.fill: parent
                        anchors.margins: 0
                        contentHeight: sideCol.implicitHeight + 24
                        clip: true
                        boundsBehavior: Flickable.StopAtBounds
                        Column {
                            id: sideCol
                            x: 10; y: 12
                            width: 220
                            spacing: 0
                            // mini month
                            Item {
                                width: parent.width; height: miniHead.height + miniGrid.height + 6
                                RowLayout {
                                    id: miniHead
                                    width: parent.width
                                    Text { text: win.monthName(win.anchor.getMonth()) + " " + win.anchor.getFullYear(); color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold; Layout.fillWidth: true }
                                    Text { text: "‹"; color: Theme.mut; font.pixelSize: 15; MouseArea { anchors.fill: parent; anchors.margins: -4; cursorShape: Qt.PointingHandCursor; onClicked: { win.anchor = new Date(win.anchor.getFullYear(), win.anchor.getMonth() - 1, 1); win.refresh() } } }
                                    Text { text: "›"; color: Theme.mut; font.pixelSize: 15; Layout.leftMargin: 6; MouseArea { anchors.fill: parent; anchors.margins: -4; cursorShape: Qt.PointingHandCursor; onClicked: { win.anchor = new Date(win.anchor.getFullYear(), win.anchor.getMonth() + 1, 1); win.refresh() } } }
                                }
                                Grid {
                                    id: miniGrid
                                    y: miniHead.height + 6
                                    columns: 7
                                    width: parent.width
                                    readonly property real cw: width / 7
                                    readonly property var cells: {
                                        const first = win.weekStart(new Date(win.anchor.getFullYear(), win.anchor.getMonth(), 1)), out = []
                                        for (let i = 0; i < 42; i++) out.push(win.addDays(first, i))
                                        return out
                                    }
                                    Repeater {
                                        model: 7
                                        delegate: Text { required property int index; width: miniGrid.cw; height: 18; horizontalAlignment: Text.AlignHCenter; text: win.dowShort((win.firstDow() + index) % 7).charAt(0); color: Theme.mut; font.pixelSize: 11 }
                                    }
                                    Repeater {
                                        model: miniGrid.cells
                                        delegate: Rectangle {
                                            required property var modelData
                                            readonly property bool isToday: win.sameDay(modelData, new Date())
                                            readonly property bool inWeek: win.view !== "month" && win.dayList.length > 0 && modelData >= win.dayList[0] && modelData <= win.addDays(win.dayList[win.dayList.length - 1], 0)
                                            readonly property bool other: modelData.getMonth() !== win.anchor.getMonth()
                                            width: miniGrid.cw; height: 22
                                            radius: 4
                                            color: isToday ? Theme.acc : (inWeek ? Theme.chip : "transparent")
                                            Text { anchors.centerIn: parent; text: modelData.getDate(); font.pixelSize: 11; font.weight: parent.isToday ? Font.DemiBold : Font.Normal; color: parent.isToday ? Theme.onAcc : (parent.other ? Theme.mut : Theme.fg) }
                                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: { win.anchor = modelData; win.refresh() } }
                                        }
                                    }
                                }
                            }
                            Text { topPadding: 16; bottomPadding: 6; text: qsTr("Upcoming").toUpperCase(); color: Theme.mut; font.pixelSize: 11; font.letterSpacing: 0.6 }
                            Repeater {
                                model: win.upcoming.slice(0, 6)
                                delegate: Rectangle {
                                    required property var modelData
                                    width: sideCol.width
                                    height: upcCol.implicitHeight + 12
                                    radius: 6
                                    color: Theme.bg
                                    Rectangle { width: 3; height: parent.height; radius: 1.5; color: modelData.color }
                                    Column {
                                        id: upcCol
                                        x: 9; y: 6
                                        width: parent.width - 14
                                        Text { width: parent.width; text: modelData.title || qsTr("(untitled)"); color: Theme.fg; elide: Text.ElideRight; font.pixelSize: 13 }
                                        Text { width: parent.width; text: (win.sameDay(new Date(modelData.startMs), new Date()) ? qsTr("today") : win.dowShort(new Date(modelData.startMs).getDay())) + " · " + win.rangeText(modelData); color: Theme.mut; font.pixelSize: 11 }
                                    }
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: { win.anchor = new Date(modelData.startMs); win.refresh(); win.select(modelData.id, modelData.startMs) } }
                                    Layout.bottomMargin: 4
                                }
                            }
                            Item { width: 1; height: 4 }
                            Text { visible: win.upcoming.length === 0; text: qsTr("Nothing more today"); color: Theme.mut; font.pixelSize: 11 }
                            Text { topPadding: 16; bottomPadding: 6; text: qsTr("Calendars").toUpperCase(); color: Theme.mut; font.pixelSize: 11; font.letterSpacing: 0.6 }
                            Repeater {
                                model: win.cals
                                delegate: Rectangle {
                                    required property var modelData
                                    width: sideCol.width; height: 26; radius: 5
                                    color: cma.containsMouse ? Theme.chip : "transparent"
                                    Row {
                                        x: 6; anchors.verticalCenter: parent.verticalCenter; spacing: 8
                                        Rectangle { width: 12; height: 12; radius: 3; border.width: 2; border.color: modelData.color; color: modelData.visible ? modelData.color : "transparent" }
                                        Text { text: modelData.name; color: Theme.fg; font.pixelSize: 13; elide: Text.ElideRight; width: 180 }
                                    }
                                    MouseArea { id: cma; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { backend.set_calendar_visible(modelData.id, !modelData.visible); win.refresh() } }
                                }
                            }
                            Item { width: 1; height: 12 }
                            Rectangle {
                                width: sideCol.width; height: 28; radius: 5
                                color: sma.containsMouse ? Theme.chip : "transparent"
                                Text { x: 6; anchors.verticalCenter: parent.verticalCenter; text: "⚙  " + qsTr("Settings"); color: Theme.mut; font.pixelSize: 12 }
                                MouseArea { id: sma; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: win.settingsOpen = true }
                            }
                        }
                    }
                }

                // ---- centre ----------------------------------------------------------------
                ColumnLayout {
                    id: centre
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumWidth: 300
                    spacing: 0

                    // header
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 48
                        Layout.leftMargin: 14
                        Layout.rightMargin: 14
                        spacing: 10
                        Btn { text: "☰"; pad: 9; onClicked: win.sideOpen = !win.sideOpen }
                        Text { text: win.titleText(); color: Theme.fg; font.pixelSize: 17; font.weight: Font.Medium; elide: Text.ElideRight; Layout.maximumWidth: 360 }
                        Btn { text: qsTr("Today"); onClicked: win.gotoToday() }
                        Btn { text: "‹"; pad: 9; onClicked: win.step(-1) }
                        Btn { text: "›"; pad: 9; onClicked: win.step(1) }
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.maximumWidth: 380
                            Layout.preferredHeight: 28
                            radius: 7
                            color: Theme.bg2
                            border.color: Theme.line
                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 10; anchors.rightMargin: 10
                                Text { text: "＋ " + qsTr("Add… e.g. “meeting tomorrow 2pm 1h”"); color: Theme.mut; font.pixelSize: 13; elide: Text.ElideRight; Layout.fillWidth: true }
                                Text { text: "Ctrl K"; color: Theme.mut; font.pixelSize: 11 }
                            }
                            MouseArea { anchors.fill: parent; cursorShape: Qt.IBeamCursor; onClicked: palette.open() }
                        }
                        Item { Layout.fillWidth: true; Layout.preferredWidth: 1 }
                        Rectangle {
                            Layout.preferredHeight: 28
                            Layout.preferredWidth: segRow.implicitWidth + 2
                            radius: 7; color: "transparent"; border.color: Theme.line; clip: true
                            Row {
                                id: segRow
                                x: 1; y: 1
                                Repeater {
                                    model: [{ k: "day", t: qsTr("Day") }, { k: "week", t: qsTr("Week") }, { k: "month", t: qsTr("Month") }]
                                    delegate: Rectangle {
                                        required property var modelData
                                        width: segText.implicitWidth + 24; height: 26
                                        color: win.view === modelData.k ? Theme.chip : "transparent"
                                        Text { id: segText; anchors.centerIn: parent; text: modelData.t; color: Theme.fg; font.pixelSize: 13; font.weight: win.view === modelData.k ? Font.DemiBold : Font.Normal }
                                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: win.setView(modelData.k) }
                                    }
                                }
                            }
                        }
                    }

                    // ----- week / day view ------------------------------------------------
                    ColumnLayout {
                        id: weekv
                        visible: win.view !== "month"
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: 0
                        readonly property real colW: Math.max(40, (width - win.gutter - win.rightPad) / Math.max(1, win.dayList.length))

                        // day names
                        Item {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 56
                            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                            Repeater {
                                model: win.view === "month" ? [] : win.dayList
                                delegate: Item {
                                    required property var modelData
                                    required property int index
                                    readonly property bool isToday: win.sameDay(modelData, new Date())
                                    x: win.gutter + index * weekv.colW; width: weekv.colW; height: 56
                                    Text { x: 0; y: 6; width: parent.width; horizontalAlignment: Text.AlignHCenter; text: win.dowShort(modelData.getDay()); color: Theme.mut; font.pixelSize: 11 }
                                    Rectangle {
                                        y: 20; anchors.horizontalCenter: parent.horizontalCenter
                                        width: 32; height: 32; radius: 16
                                        color: parent.isToday ? Theme.acc : "transparent"
                                        Text { anchors.centerIn: parent; text: modelData.getDate(); font.pixelSize: 19; font.weight: Font.Medium; color: parent.parent.isToday ? Theme.onAcc : Theme.fg }
                                    }
                                    MouseArea { anchors.fill: parent; z: -1; cursorShape: Qt.PointingHandCursor; onDoubleClicked: { win.anchor = modelData; win.setView("day") } }
                                }
                            }
                        }
                        // all-day row
                        Item {
                            id: allDayRow
                            Layout.fillWidth: true
                            readonly property int maxN: { let m = 1; for (const a of win.allDayByDay) m = Math.max(m, a.length); return m }
                            Layout.preferredHeight: 6 + maxN * 20
                            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                            Text { x: 6; y: 5; width: win.gutter - 12; horizontalAlignment: Text.AlignRight; text: qsTr("all day"); color: Theme.mut; font.pixelSize: 10; visible: win.allDayByDay.some(a => a.length > 0) }
                            Repeater {
                                model: win.allDayByDay
                                delegate: Item {
                                    required property var modelData
                                    required property int index
                                    x: win.gutter + index * weekv.colW; width: weekv.colW; height: allDayRow.height
                                    Rectangle { width: 1; height: parent.height; color: Theme.line }
                                    Column {
                                        x: 2; y: 3; width: parent.width - 4; spacing: 2
                                        Repeater {
                                            model: modelData
                                            delegate: Rectangle {
                                                required property var modelData
                                                width: parent.width; height: 18; radius: 4
                                                color: Qt.tint(Theme.bg, Qt.rgba(Qt.color(modelData.color).r, Qt.color(modelData.color).g, Qt.color(modelData.color).b, 0.25))
                                                Rectangle { width: 3; height: parent.height; color: modelData.color; radius: 1 }
                                                Text { x: 9; width: parent.width - 12; anchors.verticalCenter: parent.verticalCenter; text: modelData.title; color: Theme.fg; font.pixelSize: 11; elide: Text.ElideRight }
                                                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: win.select(modelData.id, modelData.startMs) }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // time grid
                        Flickable {
                            id: grid
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            contentHeight: 24 * win.hh + 16
                            contentWidth: width
                            clip: true
                            boundsBehavior: Flickable.StopAtBounds
                            ScrollBar.vertical: ScrollBar {
                                policy: ScrollBar.AsNeeded; width: 8
                                contentItem: Rectangle { implicitWidth: 6; radius: 3; color: parent.hovered || parent.pressed ? Theme.scrollHover : Theme.scroll }
                                background: Rectangle { color: "transparent" }
                            }
                            onContentHeightChanged: if (contentY === 0 && win.autotestArg === "") Qt.callLater(() => win.scrollToHour(win.prefs.startHour || 7))

                            Item {
                                id: gridBody
                                width: grid.width
                                height: 24 * win.hh + 16
                                // hour labels
                                Repeater {
                                    model: 23
                                    delegate: Text {
                                        required property int index
                                        x: 0; width: win.gutter - 8; horizontalAlignment: Text.AlignRight
                                        y: 8 + (index + 1) * win.hh - 7
                                        text: win.fmtHour(index + 1); color: Theme.mut; font.pixelSize: 10
                                    }
                                }
                                // day columns: background, hour lines, create-by-drag
                                Repeater {
                                    model: win.dayList
                                    delegate: Rectangle {
                                        id: dayCol
                                        required property var modelData
                                        required property int index
                                        x: win.gutter + index * weekv.colW; y: 8
                                        width: weekv.colW; height: 24 * win.hh
                                        color: (modelData.getDay() === 0 || modelData.getDay() === 6) ? Theme.bg2 : "transparent"
                                        Rectangle { width: 1; height: parent.height; color: Theme.line }
                                        Repeater { model: 24; delegate: Rectangle { required property int index; y: (index + 1) * win.hh - 1; width: dayCol.width; height: 1; color: Theme.line } }
                                        // creating: drag over empty cells
                                        MouseArea {
                                            id: createMa
                                            anchors.fill: parent
                                            property real y0: 0
                                            property real y1: 0
                                            property bool dragging: false
                                            function snap(y) { return Math.round(Math.max(0, Math.min(24 * win.hh, y)) / (win.hh / 4)) * (win.hh / 4) }
                                            onPressed: (m) => { y0 = snap(m.y); y1 = y0 + win.hh / 2; dragging = true; root.forceActiveFocus() }
                                            onPositionChanged: (m) => { if (dragging) y1 = Math.max(y0 + win.hh / 4, snap(m.y)) }
                                            readonly property bool pending: win.draftCol === dayCol.index
                                            onReleased: {
                                                if (!dragging) return
                                                dragging = false
                                                win.draftCol = dayCol.index
                                                draftIn.text = ""
                                                draftIn.forceActiveFocus()
                                            }
                                            function commitDraft() {
                                                const d = dayCol.modelData
                                                const s = new Date(d.getFullYear(), d.getMonth(), d.getDate(), 0, Math.round(y0 / win.hh * 60))
                                                const e = new Date(d.getFullYear(), d.getMonth(), d.getDate(), 0, Math.round(Math.max(y1, y0 + win.hh / 4) / win.hh * 60))
                                                const title = draftIn.text.trim()
                                                win.draftCol = -1
                                                const r = win.parse(backend.create_range(s.getTime(), e.getTime(), false))
                                                if (r && !r.error) {
                                                    if (title !== "") backend.update_event(r.id, JSON.stringify({ title: title }))
                                                    win.refresh(); win.select(r.id, r.start); win.toast(qsTr("Event created"), true)
                                                    if (title === "") win.focusTitle(); else root.forceActiveFocus()
                                                }
                                                else win.toast(r ? r.error : "", false)
                                            }
                                            onCanceled: dragging = false
                                            Connections {
                                                target: win
                                                function onAutoDraft(col, title, commit) {
                                                    if (col !== dayCol.index) return
                                                    if (!commit) { createMa.y0 = win.hh * 10; createMa.y1 = win.hh * 11; win.draftCol = col; draftIn.forceActiveFocus(); draftIn.text = title }
                                                    else createMa.commitDraft()
                                                }
                                            }
                                            Rectangle {
                                                visible: createMa.dragging || createMa.pending
                                                y: createMa.y0; height: Math.max(createMa.y1 - createMa.y0, win.hh / 4)
                                                x: 3; width: parent.width - 7
                                                radius: 6; color: Qt.rgba(Theme.sel.r, Theme.sel.g, Theme.sel.b, 0.18); border.color: Theme.sel; border.width: 2
                                                TextInput {
                                                    id: draftIn
                                                    visible: createMa.pending
                                                    x: 6; y: 3; width: parent.width - 12
                                                    color: Theme.fg; font.pixelSize: 12; font.weight: Font.DemiBold
                                                    selectionColor: Theme.sel; clip: true
                                                    Keys.onReturnPressed: createMa.commitDraft()
                                                    Keys.onEnterPressed: createMa.commitDraft()
                                                    Keys.onEscapePressed: { win.draftCol = -1; root.forceActiveFocus() }
                                                    onActiveFocusChanged: if (!activeFocus && createMa.pending && text.trim() === "") Qt.callLater(() => { if (!draftIn.activeFocus && createMa.pending) win.draftCol = -1 })
                                                    Text { visible: !draftIn.text; width: parent.width; elide: Text.ElideRight; text: qsTr("Title · Enter saves, Esc cancels"); color: Theme.mut; font.pixelSize: 11 }
                                                }
                                            }
                                        }
                                    }
                                }
                                // events
                                Item {
                                    id: evLayer
                                    x: win.gutter; y: 8
                                    width: parent.width - win.gutter; height: 24 * win.hh
                                    Repeater {
                                        model: win.placed
                                        delegate: Rectangle {
                                            id: ev
                                            required property var modelData
                                            readonly property var o: modelData.o
                                            readonly property bool isSel: win.selId === o.id
                                            readonly property real laneW: (weekv.colW - 7) / modelData.lanes
                                            property real dy: 0
                                            property int dday: 0
                                            property real dh: 0
                                            property bool busy: false
                                            readonly property real baseH: Math.max(14, (modelData.bottom - modelData.top) / 60 * win.hh - 3)
                                            readonly property bool short: baseH < 40
                                            x: (modelData.day + dday) * weekv.colW + 3 + modelData.lane * laneW
                                            width: laneW - (modelData.lanes > 1 ? 2 : 0)
                                            y: modelData.top / 60 * win.hh + 1 + dy
                                            height: baseH + dh
                                            z: busy ? 20 : (isSel ? 10 : 1)
                                            radius: 6
                                            clip: true
                                            opacity: o.end < win.nowMs && false ? 1 : ((o.endMs < win.nowMs) ? Theme.pastOpacity : 1)
                                            color: Qt.tint(Theme.bg, Qt.rgba(ec.r, ec.g, ec.b, o.tentative ? Theme.eventTint * 0.45 : Theme.eventTint))
                                            readonly property color ec: Qt.color(o.color)
                                            border.width: isSel ? 2 : 0
                                            border.color: Theme.sel
                                            Rectangle { x: 0; width: 3; height: parent.height; color: ev.ec; radius: 1 }
                                            Rectangle { visible: o.tentative; x: 3; width: parent.width - 3; height: parent.height; color: "transparent"; border.color: ev.ec; border.width: 1; radius: 6; opacity: 0.5 }
                                            Column {
                                                x: 9; y: 3
                                                width: parent.width - 14
                                                spacing: 0
                                                Text {
                                                    width: parent.width
                                                    text: (o.title || qsTr("(untitled)")) + (ev.short ? "  " : "")
                                                    color: o.title ? Theme.fg : Theme.mut
                                                    font.pixelSize: 12
                                                    font.italic: o.tentative
                                                    elide: Text.ElideRight
                                                    wrapMode: ev.short ? Text.NoWrap : Text.Wrap
                                                    maximumLineCount: ev.short ? 1 : Math.max(1, Math.floor((ev.height - 22) / 15))
                                                }
                                                Text {
                                                    visible: !ev.short || ev.height > 30
                                                    width: parent.width
                                                    text: win.rangeText(o) + (o.location && !o.location.startsWith("http") ? " · " + o.location : "") + (o.recurring ? " ↻" : "")
                                                    color: Theme.mut
                                                    font.pixelSize: 11
                                                    elide: Text.ElideRight
                                                }
                                            }
                                            MouseArea {
                                                id: moveMa
                                                anchors.fill: parent
                                                anchors.bottomMargin: 6
                                                hoverEnabled: true
                                                cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                                                property real py: 0
                                                property real px: 0
                                                property bool moved: false
                                                onPressed: (m) => { const p = mapToItem(evLayer, m.x, m.y); py = p.y; px = p.x; moved = false; ev.busy = true; win.select(o.id, o.startMs); root.forceActiveFocus() }
                                                onPositionChanged: (m) => {
                                                    if (!pressed || o.readOnly) return
                                                    const p = mapToItem(evLayer, m.x, m.y), q = win.hh / 4
                                                    ev.dy = Math.round((p.y - py) / q) * q
                                                    const col = Math.max(0, Math.min(win.dayList.length - 1, Math.floor(p.x / weekv.colW)))
                                                    ev.dday = col - modelData.day
                                                    moved = ev.dy !== 0 || ev.dday !== 0
                                                }
                                                onReleased: {
                                                    ev.busy = false
                                                    if (moved && !o.readOnly) {
                                                        const mins = Math.round(ev.dy / win.hh * 60)
                                                        const ns = new Date(o.startMs); ns.setDate(ns.getDate() + ev.dday); ns.setMinutes(ns.getMinutes() + mins)
                                                        const len = o.endMs - o.startMs
                                                        const err = backend.move_event(o.id, o.startMs, ns.getTime(), ns.getTime() + len)
                                                        ev.dy = 0; ev.dday = 0
                                                        if (err !== "") win.toast(err, false)
                                                        else { win.selOcc = ns.getTime(); win.refresh(); win.toast(qsTr("“%1” → %2 %3").arg(o.title).arg(win.dowShort(ns.getDay())).arg(win.fmtT(ns.getTime())), true) }
                                                    } else { ev.dy = 0; ev.dday = 0 }
                                                }
                                                onDoubleClicked: win.focusTitle()
                                            }
                                            MouseArea {
                                                anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                                                height: 7
                                                visible: !o.readOnly
                                                cursorShape: Qt.SizeVerCursor
                                                property real py: 0
                                                onPressed: (m) => { py = mapToItem(evLayer, m.x, m.y).y; ev.busy = true; win.select(o.id, o.startMs) }
                                                onPositionChanged: (m) => {
                                                    if (!pressed) return
                                                    const q = win.hh / 4
                                                    ev.dh = Math.max(Math.round((mapToItem(evLayer, m.x, m.y).y - py) / q) * q, q - ev.baseH)
                                                }
                                                onReleased: {
                                                    ev.busy = false
                                                    if (ev.dh !== 0) {
                                                        const ne = o.endMs + Math.round(ev.dh / win.hh * 60) * 60000
                                                        const err = backend.move_event(o.id, o.startMs, o.startMs, Math.max(ne, o.startMs + 15 * 60000))
                                                        ev.dh = 0
                                                        if (err !== "") win.toast(err, false); else { win.refresh(); win.toast(qsTr("Duration changed"), true) }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // now line
                                    Repeater {
                                        model: win.dayList
                                        delegate: Item {
                                            required property var modelData
                                            required property int index
                                            visible: win.sameDay(modelData, new Date(win.nowMs))
                                            x: index * weekv.colW; width: weekv.colW
                                            y: ((new Date(win.nowMs).getHours() * 60 + new Date(win.nowMs).getMinutes()) / 60) * win.hh
                                            z: 30
                                            Rectangle { width: parent.width; height: 2; color: Theme.nowLine; y: -1 }
                                            Rectangle { x: -5; y: -5; width: 10; height: 10; radius: 5; color: Theme.nowLine }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ----- month view ------------------------------------------------------
                    Item {
                        id: monthv
                        visible: win.view === "month"
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        readonly property real cw: width / 7
                        readonly property real ch: (height - 24) / 6
                        Repeater {
                            model: win.view === "month" ? win.dayList : []
                            delegate: Text {
                                required property var modelData
                                required property int index
                                x: index * monthv.cw; width: monthv.cw; height: 24
                                horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter
                                text: win.dowShort(modelData.getDay()); color: Theme.mut; font.pixelSize: 11
                            }
                        }
                        Rectangle { y: 24; width: parent.width; height: 1; color: Theme.line }
                        Repeater {
                            model: win.view === "month" ? win.monthCells : []
                            delegate: Rectangle {
                                id: mc
                                required property var modelData
                                required property int index
                                readonly property bool isToday: win.sameDay(modelData.date, new Date())
                                x: (index % 7) * monthv.cw; y: 24 + Math.floor(index / 7) * monthv.ch
                                width: monthv.cw; height: monthv.ch
                                color: "transparent"
                                border.color: Theme.line; border.width: 0
                                Rectangle { anchors.right: parent.right; width: 1; height: parent.height; color: Theme.line }
                                Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                opacity: modelData.other ? 0.45 : 1
                                MouseArea { anchors.fill: parent; onClicked: { win.anchor = modelData.date; root.forceActiveFocus() } onDoubleClicked: { win.anchor = modelData.date; win.setView("day") } }
                                Rectangle {
                                    x: 4; y: 3; width: 22; height: 20; radius: 10
                                    color: mc.isToday ? Theme.acc : "transparent"
                                    Text { anchors.centerIn: parent; text: modelData.date.getDate(); font.pixelSize: 11; font.weight: mc.isToday ? Font.Bold : Font.Normal; color: mc.isToday ? Theme.onAcc : Theme.mut }
                                }
                                Column {
                                    x: 4; y: 25; width: parent.width - 8; spacing: 1
                                    readonly property int fit: Math.max(1, Math.floor((mc.height - 38) / 17))
                                    Repeater {
                                        model: modelData.items.slice(0, parent.fit)
                                        delegate: Rectangle {
                                            required property var modelData
                                            width: parent.width; height: 16; radius: 3
                                            color: modelData.allDay ? Qt.tint(Theme.bg, Qt.rgba(Qt.color(modelData.color).r, Qt.color(modelData.color).g, Qt.color(modelData.color).b, 0.25)) : "transparent"
                                            Rectangle { width: 2; height: parent.height; color: modelData.color }
                                            Text { x: 6; width: parent.width - 8; anchors.verticalCenter: parent.verticalCenter; font.pixelSize: 11; elide: Text.ElideRight; color: Theme.fg; text: (modelData.allDay ? "" : win.fmtT(modelData.startMs) + " ") + modelData.title }
                                            MouseArea { anchors.fill: parent; onClicked: { win.select(modelData.id, modelData.startMs); root.forceActiveFocus() } }
                                        }
                                    }
                                    Text { visible: mc.modelData.items.length > parent.fit; text: qsTr("+%1 more").arg(mc.modelData.items.length - parent.fit); color: Theme.mut; font.pixelSize: 11; leftPadding: 6 }
                                }
                            }
                        }
                    }
                }

                // ---- right panel -------------------------------------------------------------
                Rectangle {
                    id: right
                    Layout.fillHeight: true
                    Layout.preferredWidth: win.compactRight ? 0 : 300
                    visible: !win.compactRight
                    color: Theme.panel
                    clip: true
                    Rectangle { width: 1; height: parent.height; color: Theme.line }
                    Flickable {
                        anchors.fill: parent
                        contentHeight: rightCol.implicitHeight + 32
                        boundsBehavior: Flickable.StopAtBounds
                        clip: true
                        Column {
                            id: rightCol
                            x: 16; y: 16
                            width: 268
                            spacing: 0
                            // nothing selected: next + today
                            Column {
                                visible: !win.detail
                                width: parent.width
                                Text { text: qsTr("Next").toUpperCase(); color: Theme.mut; font.pixelSize: 11; font.letterSpacing: 0.6; bottomPadding: 6 }
                                Text {
                                    visible: win.upcoming.length === 0
                                    text: qsTr("Nothing coming up"); color: Theme.mut; font.pixelSize: 13
                                }
                                Column {
                                    visible: win.upcoming.length > 0
                                    width: parent.width
                                    readonly property var nx: win.upcoming.length ? win.upcoming[0] : null
                                    Text { width: parent.width; text: parent.nx ? (parent.nx.title || qsTr("(untitled)")) : ""; color: Theme.fg; font.pixelSize: 16; font.weight: Font.DemiBold; wrapMode: Text.Wrap }
                                    Text { width: parent.width; text: parent.nx ? win.dowShort(new Date(parent.nx.startMs).getDay()) + " " + new Date(parent.nx.startMs).getDate() + " " + win.monthGen(new Date(parent.nx.startMs).getMonth()) + " · " + win.rangeText(parent.nx) : ""; color: Theme.mut; font.pixelSize: 13 }
                                    Btn { visible: parent.nx && parent.nx.link !== ""; width: parent.width; height: 34; primary: true; text: qsTr("Join the meeting"); onClicked: backend.open_url(parent.nx.link) }
                                }
                                Text { topPadding: 18; bottomPadding: 4; text: qsTr("Today — the day at a glance").toUpperCase(); color: Theme.mut; font.pixelSize: 11; font.letterSpacing: 0.6 }
                                Repeater {
                                    model: win.todayList
                                    delegate: Item {
                                        required property var modelData
                                        width: rightCol.width; height: 34
                                        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                        Rectangle { x: 0; anchors.verticalCenter: parent.verticalCenter; width: 8; height: 8; radius: 2; color: modelData.color }
                                        Text { x: 16; anchors.verticalCenter: parent.verticalCenter; width: parent.width - 70; text: modelData.title; color: Theme.fg; elide: Text.ElideRight; font.pixelSize: 13 }
                                        Text { anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; text: win.fmtT(modelData.startMs); color: Theme.mut; font.pixelSize: 13 }
                                        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: win.select(modelData.id, modelData.startMs) }
                                    }
                                }
                            }
                            // selected event
                            Column {
                                visible: !!win.detail
                                width: parent.width
                                spacing: 0
                                TextInput {
                                    id: titleIn
                                    width: parent.width
                                    text: win.detail ? win.detail.title : ""
                                    color: Theme.fg
                                    font.pixelSize: 16; font.weight: Font.DemiBold
                                    selectionColor: Theme.sel
                                    readOnly: win.detail ? win.detail.readOnly : true
                                    onEditingFinished: { if (win.detail && text !== win.detail.title) { backend.update_event(win.selId, JSON.stringify({ title: text })); win.refresh() } root.forceActiveFocus() }
                                    Text { visible: !titleIn.text && !titleIn.activeFocus; text: qsTr("Title"); color: Theme.mut; font: titleIn.font }
                                }
                                Text {
                                    visible: !!win.detail && (win.detail.allDay || win.detail.readOnly)
                                    topPadding: 4; bottomPadding: 8
                                    width: parent.width
                                    text: win.detail ? (win.detail.allDay ? win.dowShort(new Date(win.detail.startMs).getDay()) + " " + new Date(win.detail.startMs).getDate() + " " + win.monthGen(new Date(win.detail.startMs).getMonth()) + " · " + qsTr("all day")
                                        : win.dowShort(new Date(win.detail.startMs).getDay()) + " " + new Date(win.detail.startMs).getDate() + " " + win.monthGen(new Date(win.detail.startMs).getMonth()) + " · " + win.fmtT(win.detail.startMs) + "–" + win.fmtT(win.detail.endMs) + " (" + qsTr("%1 min").arg(Math.round((win.detail.endMs - win.detail.startMs) / 60000)) + ")") : ""
                                    color: Theme.mut; font.pixelSize: 13; wrapMode: Text.Wrap
                                }
                                // editable date + time range (od–do)
                                Row {
                                    id: timeRow
                                    visible: !!win.detail && !win.detail.allDay && !win.detail.readOnly
                                    topPadding: 4; bottomPadding: 8; spacing: 4
                                    function load() {
                                        dateIn.text = win.detail ? Qt.formatDate(new Date(win.detail.startMs), "dd.MM.yyyy") : ""
                                        startIn.text = win.detail ? win.fmtHM(win.detail.startMs) : ""
                                        endIn.text = win.detail ? win.fmtHM(win.detail.endMs) : ""
                                    }
                                    Component.onCompleted: load()
                                    Connections { target: win; function onDetailChanged() { timeRow.load() } }
                                    function commit() {
                                        if (!win.detail) return
                                        const s0 = new Date(win.detail.startMs), len = win.detail.endMs - win.detail.startMs
                                        const dm = /^\s*(\d{1,2})[.\/-](\d{1,2})(?:[.\/-](\d{2,4}))?\s*$/.exec(dateIn.text)
                                        const sm = win.parseHM(startIn.text), em = win.parseHM(endIn.text)
                                        if (!dm || !sm || !em) { win.toast(qsTr("Wrong date or time"), false); timeRow.load(); return }
                                        let y = dm[3] ? parseInt(dm[3]) : s0.getFullYear(); if (y < 100) y += 2000
                                        const ns = new Date(y, parseInt(dm[2]) - 1, parseInt(dm[1]), sm[0], sm[1])
                                        let ne = new Date(y, parseInt(dm[2]) - 1, parseInt(dm[1]), em[0], em[1])
                                        // start changed but end untouched -> keep duration
                                        const endTouched = endIn.text.trim() !== win.fmtHM(win.detail.endMs)
                                        if (!endTouched) ne = new Date(ns.getTime() + len)
                                        else if (ne <= ns) ne = new Date(ne.getTime() + 86400000)
                                        if (ns.getTime() === win.detail.startMs && ne.getTime() === win.detail.endMs) return
                                        const e = backend.move_event(win.selId, win.detail.startMs, ns.getTime(), ne.getTime())
                                        if (e) { win.toast(e, false); return }
                                        win.anchor = ns; win.selOcc = ns.getTime(); win.refresh(); win.select(win.selId, ns.getTime())
                                    }
                                    Text { text: win.detail ? win.dowShort(new Date(win.detail.startMs).getDay()) : ""; color: Theme.mut; font.pixelSize: 13; anchors.verticalCenter: parent.verticalCenter }
                                    TextInput { id: dateIn; color: Theme.fg; font.pixelSize: 13; selectionColor: Theme.sel; selectByMouse: true; horizontalAlignment: TextInput.AlignHCenter; onEditingFinished: { timeRow.commit(); root.forceActiveFocus() }
                                        Keys.onEscapePressed: { timeRow.load(); root.forceActiveFocus() }
                                        Rectangle { anchors.fill: parent; anchors.margins: -3; z: -1; radius: 4; color: "transparent"; border.width: 1; border.color: parent.activeFocus ? Theme.acc : Theme.line }
                                        width: 76; anchors.verticalCenter: parent.verticalCenter; }
                                    Text { text: " "; font.pixelSize: 13 }
                                    TextInput { id: startIn; color: Theme.fg; font.pixelSize: 13; selectionColor: Theme.sel; selectByMouse: true; horizontalAlignment: TextInput.AlignHCenter; onEditingFinished: { timeRow.commit(); root.forceActiveFocus() }
                                        Keys.onEscapePressed: { timeRow.load(); root.forceActiveFocus() }
                                        Rectangle { anchors.fill: parent; anchors.margins: -3; z: -1; radius: 4; color: "transparent"; border.width: 1; border.color: parent.activeFocus ? Theme.acc : Theme.line }
                                        width: 42; anchors.verticalCenter: parent.verticalCenter; }
                                    Text { text: "–"; color: Theme.mut; font.pixelSize: 13; anchors.verticalCenter: parent.verticalCenter }
                                    TextInput { id: endIn; color: Theme.fg; font.pixelSize: 13; selectionColor: Theme.sel; selectByMouse: true; horizontalAlignment: TextInput.AlignHCenter; onEditingFinished: { timeRow.commit(); root.forceActiveFocus() }
                                        Keys.onEscapePressed: { timeRow.load(); root.forceActiveFocus() }
                                        Rectangle { anchors.fill: parent; anchors.margins: -3; z: -1; radius: 4; color: "transparent"; border.width: 1; border.color: parent.activeFocus ? Theme.acc : Theme.line }
                                        width: 42; anchors.verticalCenter: parent.verticalCenter; }
                                    Text { text: win.detail ? " (" + qsTr("%1 min").arg(Math.round((win.detail.endMs - win.detail.startMs) / 60000)) + ")" : ""; color: Theme.mut; font.pixelSize: 13; anchors.verticalCenter: parent.verticalCenter }
                                }
                                // RSVP
                                Row {
                                    visible: !!win.detail && win.detail.attendees > 0
                                    spacing: 6; bottomPadding: 8
                                    Repeater {
                                        model: [{ k: "ACCEPTED", t: qsTr("Yes") }, { k: "TENTATIVE", t: qsTr("Maybe") }, { k: "DECLINED", t: qsTr("No") }]
                                        delegate: Rectangle {
                                            required property var modelData
                                            readonly property bool on: win.detail && win.detail.rsvp === modelData.k
                                            width: (rightCol.width - 12) / 3; height: 28; radius: 6
                                            color: on ? Theme.sel : Theme.bg; border.color: on ? Theme.sel : Theme.line
                                            Text { anchors.centerIn: parent; text: modelData.t; color: parent.on ? Theme.onAcc : Theme.fg; font.pixelSize: 13 }
                                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: { backend.set_rsvp(win.selId, modelData.k); win.refresh() } }
                                        }
                                    }
                                }
                                Btn { visible: !!win.detail && win.detail.link !== ""; width: parent.width; height: 34; primary: true; text: qsTr("Join the meeting"); onClicked: backend.open_url(win.detail.link) }
                                Item { width: 1; height: win.detail && win.detail.link !== "" ? 8 : 0 }
                                // calendar
                                Rectangle {
                                    width: parent.width; height: 34; color: "transparent"
                                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Calendar"); color: Theme.mut; font.pixelSize: 13 }
                                    Row {
                                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; spacing: 6
                                        Rectangle { width: 9; height: 9; radius: 2; color: win.detail ? win.detail.color : "transparent"; anchors.verticalCenter: parent.verticalCenter }
                                        Text { text: win.detail ? win.detail.calendarName : ""; color: Theme.fg; font.pixelSize: 13 }
                                    }
                                    MouseArea { anchors.fill: parent; enabled: win.detail && !win.detail.readOnly; cursorShape: Qt.PointingHandCursor; onClicked: calMenu.popup() }
                                    Menu {
                                        id: calMenu
                                        Repeater {
                                            model: win.cals.filter(c => !c.readOnly)
                                            delegate: MenuItem { required property var modelData; text: modelData.name; onTriggered: { backend.update_event(win.selId, JSON.stringify({ calendarId: modelData.id })); win.refresh() } }
                                        }
                                    }
                                }
                                // location
                                Rectangle {
                                    width: parent.width; height: 34; color: "transparent"
                                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Location"); color: Theme.mut; font.pixelSize: 13 }
                                    TextInput {
                                        id: locIn
                                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                                        width: 170; horizontalAlignment: Text.AlignRight
                                        text: win.detail ? win.detail.location : ""
                                        color: Theme.fg; font.pixelSize: 13; clip: true
                                        readOnly: win.detail ? win.detail.readOnly : true
                                        selectionColor: Theme.sel
                                        onTextChanged: cursorPosition = 0
                                        onEditingFinished: { if (win.detail && text !== win.detail.location) { backend.update_event(win.selId, JSON.stringify({ location: text })); win.refresh() } root.forceActiveFocus() }
                                        Text { visible: !locIn.text && !locIn.activeFocus; anchors.right: parent.right; text: "—"; color: Theme.fg; font: locIn.font }
                                    }
                                }
                                // attendees
                                Rectangle {
                                    width: parent.width; height: 34; color: "transparent"
                                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Attendees"); color: Theme.mut; font.pixelSize: 13 }
                                    Text { anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; text: win.detail && win.detail.attendees > 0 ? qsTr("%n people", "", win.detail.attendees) : "—"; color: Theme.fg; font.pixelSize: 13 }
                                }
                                // reminder
                                Rectangle {
                                    width: parent.width; height: 34; color: "transparent"
                                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Reminder"); color: Theme.mut; font.pixelSize: 13 }
                                    Text {
                                        anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                                        text: win.detail && win.detail.reminder !== null && win.detail.reminder !== undefined ? qsTr("%1 min before").arg(win.detail.reminder) : qsTr("none")
                                        color: Theme.fg; font.pixelSize: 13
                                    }
                                    MouseArea { anchors.fill: parent; enabled: win.detail && !win.detail.readOnly; cursorShape: Qt.PointingHandCursor; onClicked: remMenu.popup() }
                                    Menu {
                                        id: remMenu
                                        Repeater {
                                            model: [-1, 0, 5, 10, 15, 30, 60, 1440]
                                            delegate: MenuItem { required property int modelData; text: modelData < 0 ? qsTr("none") : (modelData === 0 ? qsTr("at the start") : (modelData === 1440 ? qsTr("1 day before") : qsTr("%1 min before").arg(modelData))); onTriggered: { backend.update_event(win.selId, JSON.stringify({ reminder: modelData })); win.refresh() } }
                                        }
                                    }
                                }
                                // repeat
                                Rectangle {
                                    width: parent.width; height: 34; color: "transparent"
                                    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
                                    Text { anchors.verticalCenter: parent.verticalCenter; text: qsTr("Repeat"); color: Theme.mut; font.pixelSize: 13 }
                                    Text { anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter; text: win.detail ? win.repeatLabel(win.detail.repeat) : ""; color: Theme.fg; font.pixelSize: 13 }
                                    MouseArea { anchors.fill: parent; enabled: win.detail && !win.detail.readOnly; cursorShape: Qt.PointingHandCursor; onClicked: repMenu.popup() }
                                    Menu {
                                        id: repMenu
                                        Repeater {
                                            model: ["none", "daily", "weekdays", "weekly", "monthly", "yearly"]
                                            delegate: MenuItem { required property string modelData; text: win.repeatLabel(modelData); onTriggered: { backend.update_event(win.selId, JSON.stringify({ repeat: modelData })); win.refresh() } }
                                        }
                                    }
                                }
                                Row {
                                    topPadding: 14; spacing: 6
                                    Btn { text: qsTr("Duplicate"); visible: win.detail && !win.detail.readOnly; onClicked: { const r = win.parse(backend.duplicate_event(win.selId)); if (r && !r.error) { win.anchor = new Date(r.start); win.refresh(); win.select(r.id, r.start); win.toast(qsTr("Duplicated"), true) } } }
                                    Btn { text: qsTr("Delete"); visible: win.detail && !win.detail.readOnly; onClicked: win.removeSelected() }
                                    Btn { text: qsTr("Close"); onClicked: win.select(-1, 0) }
                                }
                            }
                        }
                    }
                }
            }

            // ---- hint bar --------------------------------------------------------------------
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 24
                color: Theme.panel
                Rectangle { width: parent.width; height: 1; color: Theme.line }
                Text {
                    anchors.fill: parent; anchors.leftMargin: 12; anchors.rightMargin: 12
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                    color: Theme.mut; font.pixelSize: 11
                    text: qsTr("Drag over an empty slot = new · drag a block = move · bottom edge = length · Ctrl+K · T today · D/W/M view · Del delete")
                }
            }
        }

        // ---- toast -------------------------------------------------------------------------
        Rectangle {
            visible: win.toastMsg !== ""
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom; anchors.bottomMargin: 40
            width: toastRow.implicitWidth + 28; height: 34; radius: 8
            color: Theme.fg
            Row {
                id: toastRow
                anchors.centerIn: parent; spacing: 14
                Text { text: win.toastMsg; color: Theme.bg; font.pixelSize: 13 }
                Text { visible: win.toastUndo; text: qsTr("Undo"); color: Theme.dark ? "#8a6d1c" : Theme.acc; font.pixelSize: 13; font.underline: true; MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: win.doUndo() } }
            }
        }

        // ---- quick add palette ----------------------------------------------------------------
        Rectangle {
            id: palette
            visible: win.paletteOpen
            anchors.fill: parent
            color: "#59000000"
            z: 100
            property var pv: null
            function open() { win.paletteOpen = true; pin.text = ""; pv = null; pin.forceActiveFocus() }
            function close() { win.paletteOpen = false; root.forceActiveFocus() }
            MouseArea { anchors.fill: parent; onClicked: palette.close() }
            Rectangle {
                x: (parent.width - width) / 2; y: parent.height * 0.14
                width: 560; height: boxCol.implicitHeight
                radius: 12; color: Theme.bg; border.color: Theme.line; clip: true
                MouseArea { anchors.fill: parent }
                Column {
                    id: boxCol
                    width: parent.width
                    TextField {
                        id: pin
                        width: parent.width; height: 52
                        leftPadding: 18; rightPadding: 18
                        font.pixelSize: 15
                        color: Theme.fg
                        placeholderText: qsTr("Type an event or a command…")
                        placeholderTextColor: Theme.mut
                        selectionColor: Theme.sel
                        background: Rectangle { color: "transparent"; Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line } }
                        onTextChanged: palette.pv = text.trim() === "" ? null : win.parse(backend.preview_json(text))
                        onAccepted: { if (text.trim() !== "") win.createFromText(text) }
                        Keys.onEscapePressed: palette.close()
                    }
                    Flow {
                        width: parent.width - 36; x: 18
                        topPadding: 12; bottomPadding: 12; spacing: 6
                        Text { visible: !palette.pv; text: qsTr("e.g. “Project review on Friday 10:30 45 min weekly”"); color: Theme.mut; font.pixelSize: 12 }
                        Repeater {
                            model: palette.pv ? win.chips(palette.pv) : []
                            delegate: Rectangle {
                                required property var modelData
                                height: 24; width: chipT.implicitWidth + 18; radius: 12; color: Theme.chip
                                Text { id: chipT; anchors.centerIn: parent; text: modelData; color: Theme.fg; font.pixelSize: 12 }
                            }
                        }
                    }
                    Repeater {
                        model: [
                            { t: qsTr("Create event"), k: "Enter", a: "create" },
                            { t: qsTr("Show month"), k: "M", a: "month" },
                            { t: qsTr("Toggle side panel"), k: "Ctrl \\", a: "side" },
                            { t: qsTr("Go to today"), k: "T", a: "today" },
                            { t: qsTr("Settings"), k: "Ctrl ,", a: "settings" }
                        ]
                        delegate: Rectangle {
                            required property var modelData
                            required property int index
                            width: boxCol.width; height: 36
                            color: (index === 0 || cma2.containsMouse) ? Theme.chip : "transparent"
                            Text { x: 18; anchors.verticalCenter: parent.verticalCenter; text: modelData.t; color: Theme.fg; font.pixelSize: 13 }
                            Text { anchors.right: parent.right; anchors.rightMargin: 18; anchors.verticalCenter: parent.verticalCenter; text: modelData.k; color: Theme.mut; font.family: "monospace"; font.pixelSize: 11 }
                            MouseArea {
                                id: cma2; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    if (modelData.a === "create") { if (pin.text.trim() !== "") win.createFromText(pin.text) }
                                    else { palette.close(); if (modelData.a === "month") win.setView("month"); else if (modelData.a === "side") win.sideOpen = !win.sideOpen; else if (modelData.a === "today") win.gotoToday(); else win.settingsOpen = true }
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- settings ------------------------------------------------------------------------
        Loader {
            id: settingsLoader
            anchors.fill: parent
            z: 200
            active: win.settingsOpen
            sourceComponent: Settings {
                be: backend
                prefs: win.prefs
                accounts: win.accounts
                lang: win.lang
                onCloseRequested: { win.settingsOpen = false; win.refresh(); root.forceActiveFocus() }
                onChanged: win.refresh()
            }
        }

        // ---- window edges (frameless) ----------------------------------------------------------
        Repeater {
            model: [
                { e: Qt.LeftEdge, l: true, t: false, r: false, b: false, c: Qt.SizeHorCursor },
                { e: Qt.RightEdge, l: false, t: false, r: true, b: false, c: Qt.SizeHorCursor },
                { e: Qt.TopEdge, l: false, t: true, r: false, b: false, c: Qt.SizeVerCursor },
                { e: Qt.BottomEdge, l: false, t: false, r: false, b: true, c: Qt.SizeVerCursor },
                { e: Qt.LeftEdge | Qt.TopEdge, l: true, t: true, r: false, b: false, c: Qt.SizeFDiagCursor },
                { e: Qt.RightEdge | Qt.BottomEdge, l: false, t: false, r: true, b: true, c: Qt.SizeFDiagCursor },
                { e: Qt.RightEdge | Qt.TopEdge, l: false, t: true, r: true, b: false, c: Qt.SizeBDiagCursor },
                { e: Qt.LeftEdge | Qt.BottomEdge, l: true, t: false, r: false, b: true, c: Qt.SizeBDiagCursor }
            ]
            delegate: MouseArea {
                required property var modelData
                visible: win.visibility === Window.Windowed
                z: 500
                width: (modelData.l || modelData.r) ? 5 : parent.width - 10
                height: (modelData.t || modelData.b) ? 5 : parent.height - 10
                x: modelData.l ? 0 : (modelData.r ? parent.width - 5 : 5)
                y: modelData.t ? 0 : (modelData.b ? parent.height - 5 : 5)
                cursorShape: modelData.c
                onPressed: win.startSystemResize(modelData.e)
            }
        }
    }

    // ---- actions used above ----------------------------------------------------------------------
    function focusTitle() { Qt.callLater(() => { titleIn.forceActiveFocus(); titleIn.selectAll() }) }
    function removeSelected() {
        const t = backend.delete_event(selId)
        if (t.startsWith("!")) { toast(t.substring(1), false); return }
        select(-1, 0)
        refresh()
        toast(qsTr("Deleted “%1”").arg(t), true)
    }
    function chips(p) {
        const out = []
        out.push("📌 " + p.title)
        const d = new Date(p.date + "T00:00:00")
        out.push("📅 " + dowShort(d.getDay()) + " " + d.getDate() + " " + monthGen(d.getMonth()).substring(0, 3))
        out.push("🕐 " + (p.hasTime ? p.start + (p.duration ? " (" + p.duration + " min)" : "") : (p.allDay ? qsTr("all day") : "")))
        if (p.repeat) out.push("🔁 " + repeatLabel(p.repeat))
        if (p.location) out.push("📍 " + p.location)
        if (p.calendar) out.push("# " + p.calendar)
        if (p.attendees && p.attendees.length) out.push("👥 " + p.attendees.length)
        return out
    }

    // ---- scripted check: --autotest=DIR -----------------------------------------------------------
    property int autoStep: 0
    function shot(name) {
        const root_ = root
        root_.grabToImage(function (r) { r.saveToFile(win.autotestArg + "/" + name + ".png"); console.warn("AUTOTEST shot", name) })
    }
    Timer {
        id: autoTimer
        interval: 1200
        repeat: true
        onTriggered: {
            const s = win.autoStep++
            if (s === 0) { Theme.name = "light"; win.scrollToHour(7); win.shot("week-light") }
            else if (s === 1) { const o = win.placed.find(p => p.o.title.indexOf("Przegl") === 0 || p.o.title.indexOf("Project") === 0); if (o) win.select(o.o.id, o.o.startMs); console.warn("AUTOTEST placed", win.placed.length) }
            else if (s === 2) { win.shot("week-selected"); if (win.detail) { const b = win.fmtHM(win.detail.startMs) + "-" + win.fmtHM(win.detail.endMs); startIn.text = "9:15"; endIn.text = "11:45"; timeRow.commit(); console.warn("AUTOTEST timeedit", b, "->", win.detail ? win.fmtHM(win.detail.startMs) + "-" + win.fmtHM(win.detail.endMs) : "brak", timeRow.visible); win.shot("time-edited") } }
            else if (s === 3) { win.autoDraft(3, "Szkic testowy", false); win.shot("draft-pending") }
            else if (s === 4) { win.autoDraft(3, "", true); win.shot("draft-saved") }
            else if (s === 5) { palette.open(); pin.text = win.isPl ? "Rozmowa o budżecie w poniedziałek 15:30 45 min co tydzień" : "Budget talk monday 3:30pm 45 min weekly" }
            else if (s === 6) { win.shot("palette") }
            else if (s === 7) { const ok = win.createFromText(pin.text); console.warn("AUTOTEST create", ok, win.placed.length) }
            else if (s === 8) { win.shot("week-created") }
            else if (s === 9) { win.setView("month") }
            else if (s === 10) { win.shot("month") }
            else if (s === 11) { win.anchor = new Date(); win.setView("day") }
            else if (s === 12) { win.shot("day") }
            else if (s === 13) { win.setView("week"); win.settingsOpen = true }
            else if (s === 14) { win.shot("settings"); if (settingsLoader.item) { settingsLoader.item.tab = "accounts"; settingsLoader.item.adding = true } }
            else if (s === 15) { win.shot("settings-accounts") }
            else if (s === 16) { if (settingsLoader.item) { settingsLoader.item.tab = "general"; settingsLoader.item.adding = false }; Theme.name = "navy"; win.shot("settings-navy") }
            else if (s === 17) { win.settingsOpen = false; Theme.name = "dark"; win.select(-1, 0); win.scrollToHour(Math.max(0, Math.min(13, new Date().getHours() - 9))) }
            else if (s === 18) { win.shot("week-dark") }
            else if (s === 19) { Theme.name = "navy"; win.scrollToHour(Math.max(0, Math.min(13, new Date().getHours() - 9))) }
            else if (s === 20) { win.shot("week-navy") }
            else if (s === 21) { win.doUndo(); console.warn("AUTOTEST undo", win.placed.length) }
            else if (s === 22) { console.warn("AUTOTEST done"); Qt.quit() }
        }
    }
}
