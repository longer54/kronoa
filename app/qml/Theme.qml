pragma Singleton
import QtQuick

// Three palettes. light = mock-up A ("Notion-like"). navy and dark are the two dark palettes of the
// owner's mail client Kinto (kinto-oss app/src/theme.rs): navy = current built-in DARK (navy + gold),
// dark = its original neutral-grey DARK (blue accent). Derived roles use Kinto's palette() mixes:
// border = mix(bg, fg, .14), section = mix(bg, fg, .03), selected = mix(bg, accent, .22).
QtObject {
    // "light" | "dark" | "navy"
    property string name: "light"
    readonly property bool dark: name !== "light"
    readonly property bool navy: name === "navy"
    function pick(l, d, n) { return name === "navy" ? n : (name === "dark" ? d : l) }

    readonly property color bg: pick("#ffffff", "#1e1f24", "#151728")
    readonly property color bg2: pick("#f7f7f5", "#24252a", "#1b1d2d")
    readonly property color line: pick("#ececea", "#45464c", "#363849")
    readonly property color fg: pick("#37352f", "#dcdde3", "#e6e2d3")
    readonly property color mut: pick("#787774", "#a9acb6", "#aaa697")
    readonly property color acc: pick("#e16259", "#6b9bff", "#c9a84c")
    readonly property color sel: pick("#2383e2", "#6b9bff", "#c9a84c")
    readonly property color panel: pick("#fbfbfa", "#18191d", "#10121f")
    readonly property color chip: pick("#efefed", "#25272d", "#1d2036")
    readonly property color onAcc: pick("#ffffff", "#1e1f24", "#151728")
    // scrollbars, past events, current-time line
    readonly property color scroll: pick("#c9ccd4", "#66676d", "#646469")
    readonly property color scrollHover: pick("#a9acb7", "#8c8d93", "#8e8d8b")
    readonly property real pastOpacity: dark ? 0.86 : 0.78
    readonly property real eventTint: dark ? 0.34 : 0.22
    readonly property color nowLine: pick("#e16259", "#ff6b6b", "#ff6b6b")
    readonly property string font: "Inter"
}
