import QtQuick
import QtQuick.Controls

// Themed single-line text field.
TextField {
    color: Theme.fg
    placeholderTextColor: Theme.mut
    selectionColor: Theme.sel
    selectedTextColor: Theme.onAcc
    font.pixelSize: 13
    leftPadding: 8
    rightPadding: 8
    topPadding: 5
    bottomPadding: 5
    background: Rectangle {
        radius: 6
        color: Theme.bg2
        border.color: parent.activeFocus ? Theme.sel : Theme.line
        border.width: 1
    }
}
