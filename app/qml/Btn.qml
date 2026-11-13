import QtQuick

// Small flat button of the mock-up (.btn); `primary` = accent fill (.join), `on` = pressed segment.
Rectangle {
    id: b
    property string text: ""
    property bool primary: false
    property bool on: false
    property int pad: 10
    signal clicked()
    implicitWidth: label.implicitWidth + pad * 2
    implicitHeight: 28
    radius: 6
    color: primary ? Theme.acc : (on ? Theme.chip : (ma.containsMouse ? Theme.chip : Theme.bg))
    border.color: primary ? Theme.acc : Theme.line
    border.width: 1
    Text {
        id: label
        anchors.centerIn: parent
        text: b.text
        color: b.primary ? Theme.onAcc : Theme.fg
        font.pixelSize: 13
        font.weight: b.primary || b.on ? Font.DemiBold : Font.Normal
    }
    MouseArea { id: ma; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: b.clicked() }
}
