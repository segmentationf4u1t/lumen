import QtQuick
import QtQuick.Effects
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

// lumen panel for a Quickshell bar: live keyboard preview, presets, animation, brightness, pause and
// autostart. Talks to vulcand over $XDG_RUNTIME_DIR/lumen.sock (see src/control.rs for the protocol).
// Self-contained: only QtQuick and Quickshell imports, dark-volcanic palette, JetBrainsMono Nerd Font
// and the tabler-icons webfont. Load it in a dropdown/popup; a host may set `dropdown` (with close()).
Item {
    id: root

    property var dropdown

    // ---- palette (dark-volcanic) ------------------------------------------------------------------
    readonly property QtObject pal: QtObject {
        readonly property color basalt: "#100908"
        readonly property color crater: "#1A100E"
        readonly property color lava: "#261513"
        readonly property color obsidian: "#3A201C"
        readonly property color magma: "#FF7A3D"
        readonly property color sulfur: "#F3C24F"
        readonly property color ember: "#FFA36B"
        readonly property color ash: "#8F7D74"
        readonly property color ashDim: "#6B5A52"
        readonly property color moss: "#4FC48A"
        readonly property color fire: "#FF5C5C"
        readonly property color text: "#ebdfd8"
    }
    readonly property string textFont: "JetBrainsMono Nerd Font Propo"
    readonly property string iconFont: "tabler-icons"
    readonly property int animFast: 140

    function alpha(c, a) {
        return Qt.rgba(c.r, c.g, c.b, a);
    }

    // tabler-icons 3.x codepoints
    function icon(name) {
        const cp = {
            "keyboard": 0xebd6, "player-pause": 0xed45, "player-play": 0xed46, "sun": 0xeb30,
            "refresh": 0xeb13, "edit": 0xea98, "folder": 0xeaad, "power": 0xeb0d, "sparkles": 0xf6d7,
            "color-swatch": 0xeb61, "wave-sine": 0xecd4, "alert-triangle": 0xea06
        }[name];
        return cp === undefined ? "" : String.fromCodePoint(cp);
    }

    // ---- daemon connection ------------------------------------------------------------------------
    property var st: null              // last "state" message
    property var keys: []              // layout keys: {index, name, x, y, w}
    property real boardWidth: 16
    property real boardHeight: 5
    property var frame: []             // "#rrggbb" per LED index
    readonly property bool online: sock.connected && st !== null

    function send(obj) {
        if (!sock.connected)
            return;
        sock.write(JSON.stringify(obj) + "\n");
        sock.flush();
    }

    function receive(line) {
        let msg;
        try {
            msg = JSON.parse(line);
        } catch (e) {
            return;
        }
        if (msg.type === "frame") {
            const hex = msg.rgb;
            const out = new Array(hex.length / 6);
            for (let i = 0; i < out.length; i++)
                out[i] = "#" + hex.substr(i * 6, 6);
            frame = out;
        } else if (msg.type === "state") {
            st = msg;
        } else if (msg.type === "layout") {
            boardWidth = msg.width;
            boardHeight = msg.height;
            keys = msg.keys;
        }
    }

    Socket {
        id: sock
        path: Quickshell.env("XDG_RUNTIME_DIR") + "/lumen.sock"
        connected: true
        parser: SplitParser {
            onRead: line => root.receive(line)
        }
        onConnectionStateChanged: {
            if (connected)
                root.send({ cmd: "frames", on: true });
            else
                root.st = null;
        }
    }

    // vulcand not running yet (or restarting): keep trying while the panel is open.
    Timer {
        interval: 1500
        repeat: true
        running: !sock.connected
        onTriggered: {
            sock.connected = false;
            sock.connected = true;
        }
    }

    // Slider drags: at most ~20 brightness updates a second, the last value always lands.
    property real pendingBrightness: -1
    Timer {
        id: brightnessThrottle
        interval: 50
        onTriggered: {
            if (root.pendingBrightness >= 0)
                root.send({ cmd: "brightness", value: root.pendingBrightness });
            root.pendingBrightness = -1;
        }
    }
    function setBrightness(v) {
        pendingBrightness = Math.max(0.05, Math.min(1, v));
        if (!brightnessThrottle.running)
            brightnessThrottle.start();
    }

    // ---- small building blocks ----------------------------------------------------------------------
    component Txt: Text {
        property bool muted: false
        property bool small: false
        color: muted ? root.pal.ash : root.pal.text
        font.family: root.textFont
        font.pixelSize: small ? 12 : 14
        font.weight: Font.Medium
        elide: Text.ElideRight
        verticalAlignment: Text.AlignVCenter
    }

    component Glyph: Text {
        property int size: 16
        color: root.pal.text
        font.family: root.iconFont
        font.pixelSize: size
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    component Section: RowLayout {
        id: section
        property string title: ""
        default property alias trailing: trailingRow.data
        Layout.fillWidth: true
        spacing: 8
        Txt {
            Layout.fillWidth: true
            text: section.title.toUpperCase()
            small: true
            muted: true
            font.letterSpacing: 1
        }
        RowLayout {
            id: trailingRow
            spacing: 6
        }
    }

    // Muted text that turns magma on hover (and stays magma while `active`).
    component Link: Txt {
        id: link
        property bool active: false
        signal clicked
        small: true
        color: active || linkHover.hovered ? root.pal.magma : root.pal.ash
        Behavior on color { ColorAnimation { duration: root.animFast } }
        HoverHandler {
            id: linkHover
            cursorShape: Qt.PointingHandCursor
        }
        TapHandler {
            onTapped: link.clicked()
        }
    }

    component RoundButton: Rectangle {
        id: rb
        property string glyph: ""
        property bool checked: false
        signal clicked
        implicitWidth: 32
        implicitHeight: 32
        radius: width / 2
        color: checked ? root.alpha(root.pal.magma, 0.18) : rbArea.containsMouse ? root.pal.lava : "transparent"
        border.width: checked ? 1 : 0
        border.color: root.alpha(root.pal.magma, 0.5)
        Behavior on color { ColorAnimation { duration: root.animFast } }
        Glyph {
            anchors.centerIn: parent
            text: rb.glyph
            color: rb.checked ? root.pal.magma : root.pal.text
        }
        MouseArea {
            id: rbArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: rb.clicked()
        }
    }

    component Switch: Item {
        id: sw
        property bool checked: false
        signal toggled(bool checked)
        implicitWidth: 38
        implicitHeight: 22
        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: sw.checked ? root.pal.magma : root.pal.obsidian
            Behavior on color { ColorAnimation { duration: root.animFast } }
            Rectangle {
                width: parent.height - 6
                height: width
                radius: width / 2
                anchors.verticalCenter: parent.verticalCenter
                x: sw.checked ? parent.width - width - 3 : 3
                color: sw.checked ? root.pal.basalt : root.pal.ash
                Behavior on x { NumberAnimation { duration: root.animFast; easing.type: Easing.OutCubic } }
            }
        }
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: sw.toggled(!sw.checked)
        }
    }

    // Value 0..1; stateless: bind `value`, apply `moved(v)`.
    component Track: Item {
        id: track
        property real value: 0
        signal moved(real value)
        readonly property real position: trackArea.pressed ? trackArea.dragValue : Math.max(0, Math.min(1, value))
        implicitHeight: 22
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width
            height: 6
            radius: 3
            color: root.pal.obsidian
            Rectangle {
                width: Math.max(parent.height, parent.width * track.position)
                height: parent.height
                radius: parent.radius
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0; color: root.pal.ember }
                    GradientStop { position: 1; color: root.pal.magma }
                }
            }
        }
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            x: (track.width - width) * track.position
            width: 16
            height: 16
            radius: 8
            color: root.pal.text
            border.width: 2
            border.color: root.pal.magma
            scale: trackArea.pressed || trackArea.containsMouse ? 1.15 : 1
            Behavior on scale { NumberAnimation { duration: root.animFast } }
        }
        MouseArea {
            id: trackArea
            property real dragValue: 0
            function update(x) {
                dragValue = Math.max(0, Math.min(1, x / track.width));
                track.moved(dragValue);
            }
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            preventStealing: true
            onPressed: mouse => update(mouse.x)
            onPositionChanged: mouse => {
                if (pressed)
                    update(mouse.x);
            }
            onWheel: wheel => track.moved(Math.max(0, Math.min(1, track.value + (wheel.angleDelta.y > 0 ? 0.05 : -0.05))))
        }
    }

    // Six colors that suggest what a preset looks like.
    function swatch(p) {
        const rainbow = ["#ff3b3b", "#ffb13b", "#7dff5a", "#3bd8ff", "#5a6bff", "#e05aff"];
        switch (p.effect) {
        case "static":
            return [p.color, p.color, p.color, p.color, p.color, p.color];
        case "breathe":
            return [p.color, Qt.darker(p.color, 1.4), Qt.darker(p.color, 2.2), Qt.darker(p.color, 2.2), Qt.darker(p.color, 1.4), p.color];
        case "wave": {
            const mid = Qt.tint(p.color, root.alpha(p.secondary, 0.5));
            return [p.color, mid, p.secondary, p.secondary, mid, p.color];
        }
        case "fire":
            return ["#ff2a00", "#ff5a00", "#ff8a00", "#ffb000", "#ff6a00", "#ff2a00"];
        default:
            return rainbow;
        }
    }

    component PresetCard: Rectangle {
        id: card
        required property var preset
        readonly property bool selected: root.st !== null && root.st.profile === preset.name
        readonly property var stops: root.swatch(preset)

        Layout.fillWidth: true
        implicitHeight: 54
        radius: 12
        color: selected ? root.alpha(root.pal.magma, 0.12) : cardArea.containsMouse ? root.pal.lava : root.pal.crater
        border.width: 1
        border.color: selected ? root.pal.magma : root.pal.obsidian
        Behavior on color { ColorAnimation { duration: root.animFast } }
        Behavior on border.color { ColorAnimation { duration: root.animFast } }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 10
            anchors.rightMargin: 12
            spacing: 10

            Rectangle {
                implicitWidth: 34
                implicitHeight: 34
                radius: 9
                opacity: 0.45 + 0.55 * card.preset.brightness
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0.0; color: card.stops[0] }
                    GradientStop { position: 0.2; color: card.stops[1] }
                    GradientStop { position: 0.4; color: card.stops[2] }
                    GradientStop { position: 0.6; color: card.stops[3] }
                    GradientStop { position: 0.8; color: card.stops[4] }
                    GradientStop { position: 1.0; color: card.stops[5] }
                }
                // sheen
                Rectangle {
                    anchors.fill: parent
                    radius: parent.radius
                    gradient: Gradient {
                        GradientStop { position: 0; color: Qt.rgba(1, 1, 1, 0.18) }
                        GradientStop { position: 0.55; color: Qt.rgba(1, 1, 1, 0) }
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                Txt {
                    Layout.fillWidth: true
                    text: card.preset.name
                    font.weight: card.selected ? Font.Bold : Font.Medium
                    color: card.selected ? root.pal.text : root.pal.text
                }
                Txt {
                    Layout.fillWidth: true
                    text: card.preset.effect + " · " + Math.round(card.preset.brightness * 100) + "%"
                    small: true
                    muted: true
                }
            }

            Rectangle {
                implicitWidth: 8
                implicitHeight: 8
                radius: 4
                color: root.pal.magma
                opacity: card.selected ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: root.animFast } }
            }
        }

        MouseArea {
            id: cardArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.send({ cmd: "profile", name: card.preset.name })
        }
    }

    component Pill: Rectangle {
        id: pill
        property string label: ""
        property bool selected: false
        signal clicked
        implicitWidth: pillText.implicitWidth + 26
        implicitHeight: 30
        radius: 15
        color: selected ? root.pal.magma : pillArea.containsMouse ? root.pal.lava : root.pal.crater
        border.width: selected ? 0 : 1
        border.color: root.pal.obsidian
        Behavior on color { ColorAnimation { duration: root.animFast } }
        Txt {
            id: pillText
            anchors.centerIn: parent
            text: pill.label
            small: true
            color: pill.selected ? root.pal.basalt : root.pal.text
            font.weight: pill.selected ? Font.Bold : Font.Medium
        }
        MouseArea {
            id: pillArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: pill.clicked()
        }
    }

    // ---- layout -------------------------------------------------------------------------------------
    implicitWidth: 460
    implicitHeight: col.implicitHeight

    ColumnLayout {
        id: col
        width: parent.width
        spacing: 16

        // header
        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            Rectangle {
                implicitWidth: 40
                implicitHeight: 40
                radius: 12
                color: root.pal.lava
                border.width: 1
                border.color: root.pal.obsidian
                Glyph {
                    anchors.centerIn: parent
                    text: root.icon("keyboard")
                    size: 22
                    color: root.pal.magma
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Txt {
                    text: "lumen"
                    font.pixelSize: 16
                    font.weight: Font.Bold
                }
                RowLayout {
                    spacing: 6
                    Rectangle {
                        implicitWidth: 7
                        implicitHeight: 7
                        radius: 4
                        color: !root.online ? root.pal.fire
                            : root.st.paused ? root.pal.ash
                            : root.st.connected ? root.pal.moss : root.pal.sulfur
                    }
                    Txt {
                        Layout.fillWidth: true
                        small: true
                        muted: true
                        text: !root.online ? "vulcand is not running"
                            : "Vulcan II Mini · " + root.st.status
                    }
                }
            }

            RoundButton {
                visible: root.online
                glyph: root.icon(root.online && root.st.paused ? "player-play" : "player-pause")
                checked: root.online && root.st.paused
                onClicked: root.send({ cmd: "paused", on: !root.st.paused })
            }
        }

        // live preview
        Rectangle {
            id: boardFrame
            Layout.fillWidth: true
            readonly property real pad: 12
            readonly property real unit: (width - 2 * pad) / root.boardWidth
            implicitHeight: unit * root.boardHeight + 2 * pad
            radius: 14
            color: root.pal.basalt
            border.width: 1
            border.color: root.pal.obsidian
            clip: true

            // Bloom: the lit keys, blurred, under the crisp ones. Declared first so it draws below
            // them (a negative z would put it under the board's own background).
            MultiEffect {
                source: keyLayer
                anchors.fill: keyLayer
                opacity: keyLayer.opacity * 0.9
                blurEnabled: true
                blur: 1.0
                blurMax: 32
                saturation: 0.2
            }

            Item {
                id: keyLayer
                x: boardFrame.pad
                y: boardFrame.pad
                width: boardFrame.width - 2 * boardFrame.pad
                height: boardFrame.unit * root.boardHeight
                opacity: root.online && !root.st.paused ? 1 : 0.35
                Behavior on opacity { NumberAnimation { duration: 220 } }

                Repeater {
                    model: root.keys
                    delegate: Rectangle {
                        required property var modelData
                        readonly property real gap: boardFrame.unit * 0.12
                        x: modelData.x * boardFrame.unit + gap / 2
                        y: modelData.y * boardFrame.unit + gap / 2
                        width: modelData.w * boardFrame.unit - gap
                        height: boardFrame.unit - gap
                        radius: boardFrame.unit * 0.2
                        color: root.frame[modelData.index] ?? root.pal.lava
                        // keycap depth: light from above
                        Rectangle {
                            anchors.fill: parent
                            radius: parent.radius
                            gradient: Gradient {
                                GradientStop { position: 0; color: Qt.rgba(1, 1, 1, 0.16) }
                                GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, 0) }
                                GradientStop { position: 1; color: Qt.rgba(0, 0, 0, 0.22) }
                            }
                        }
                    }
                }
            }

            Rectangle {
                anchors.centerIn: parent
                visible: !root.online || root.st.paused
                implicitWidth: overlayText.implicitWidth + 28
                implicitHeight: 32
                radius: 16
                color: root.alpha(root.pal.basalt, 0.85)
                border.width: 1
                border.color: root.pal.obsidian
                Txt {
                    id: overlayText
                    anchors.centerIn: parent
                    small: true
                    text: !root.online ? "waiting for vulcand…" : "paused"
                }
            }
        }

        // app rule in charge
        Rectangle {
            Layout.fillWidth: true
            visible: root.online && root.st.app !== null
            implicitHeight: 34
            radius: 10
            color: root.alpha(root.pal.sulfur, 0.10)
            border.width: 1
            border.color: root.alpha(root.pal.sulfur, 0.35)
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                spacing: 8
                Glyph {
                    text: root.icon("sparkles")
                    color: root.pal.sulfur
                }
                Txt {
                    Layout.fillWidth: true
                    small: true
                    text: root.online && root.st.app ? root.st.app + " → " + root.st.activeProfile : ""
                }
                Txt {
                    small: true
                    muted: true
                    text: "app rule"
                }
            }
        }

        // presets
        ColumnLayout {
            Layout.fillWidth: true
            visible: root.online
            spacing: 8

            Section {
                title: "Presets"
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 8
                rowSpacing: 8
                Repeater {
                    model: root.online ? root.st.profiles : []
                    delegate: PresetCard {
                        required property var modelData
                        preset: modelData
                    }
                }
            }
        }

        // animation
        ColumnLayout {
            Layout.fillWidth: true
            visible: root.online
            spacing: 8

            Section {
                title: "Animation"
                Link {
                    text: "from preset"
                    active: root.online && root.st.effect === null
                    onClicked: root.send({ cmd: "effect", name: null })
                }
            }

            Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                    model: root.online ? root.st.effects : []
                    delegate: Pill {
                        required property var modelData
                        label: modelData.label.replace(/ \(.*\)$/, "")
                        selected: root.st.activeEffect === modelData.id
                        onClicked: root.send({ cmd: "effect", name: modelData.id })
                    }
                }
            }
        }

        // brightness
        ColumnLayout {
            Layout.fillWidth: true
            visible: root.online
            spacing: 6

            Section {
                title: "Brightness"
                Txt {
                    small: true
                    text: root.online ? Math.round(root.st.activeBrightness * 100) + "%" : ""
                }
                Link {
                    text: "from preset"
                    active: root.online && root.st.brightness === null
                    onClicked: root.send({ cmd: "brightness", value: null })
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Glyph {
                    text: root.icon("sun")
                    color: root.pal.ash
                    size: 14
                }
                Track {
                    Layout.fillWidth: true
                    value: root.online ? root.st.activeBrightness : 0
                    onMoved: v => root.setBrightness(v)
                }
                Glyph {
                    text: root.icon("sun")
                    color: root.pal.magma
                    size: 18
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            visible: root.online
            implicitHeight: 1
            color: root.pal.obsidian
        }

        // footer
        RowLayout {
            Layout.fillWidth: true
            visible: root.online
            spacing: 10

            Txt {
                text: "Start on login"
            }
            Switch {
                checked: root.online && root.st.autostart
                onToggled: on => root.send({ cmd: "autostart", on: on })
            }
            Item {
                Layout.fillWidth: true
            }
            Link {
                text: "Edit config"
                onClicked: {
                    root.send({ cmd: "edit_config" });
                    if (root.dropdown)
                        root.dropdown.close();
                }
            }
            Link {
                text: "Reload"
                onClicked: root.send({ cmd: "reload" })
            }
        }

        // daemon not running
        RowLayout {
            Layout.fillWidth: true
            visible: !root.online
            spacing: 10
            Txt {
                Layout.fillWidth: true
                small: true
                muted: true
                wrapMode: Text.WordWrap
                text: "Start vulcand, or turn on “Start on login” once it runs."
            }
            Link {
                text: "Start vulcand"
                onClicked: Quickshell.execDetached(["sh", "-c", "systemctl --user start vulcand.service 2>/dev/null || vulcand"])
            }
        }
    }
}
