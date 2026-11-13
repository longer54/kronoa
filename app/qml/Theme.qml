pragma Singleton
import QtQuick

// Colours of mock-up A: light "Notion-like" and the navy/gold dark variant.
QtObject {
    property bool dark: false
    readonly property color bg: dark ? "#151728" : "#ffffff"
    readonly property color bg2: dark ? "#1b1e33" : "#f7f7f5"
    readonly property color line: dark ? "#272a44" : "#ececea"
    readonly property color fg: dark ? "#e9e7df" : "#37352f"
    readonly property color mut: dark ? "#a9abc4" : "#787774"
    readonly property color acc: dark ? "#C9A84C" : "#e16259"
    readonly property color sel: dark ? "#C9A84C" : "#2383e2"
    readonly property color panel: dark ? "#181a2e" : "#fbfbfa"
    readonly property color chip: dark ? "#23263f" : "#efefed"
    readonly property color onAcc: dark ? "#151728" : "#ffffff"
    readonly property string font: "Inter"
}
