import SwiftUI
import TickerCore

private typealias Settings = TickerCore.Settings

struct SettingsView: View {
    let model: AppModel

    var body: some View {
        let settings = model.overlay.settings
        Form {
            Section("Text") {
                LabeledContent("Size") {
                    HStack {
                        Slider(value: binding(\.fontSize), in: Settings.fontSizeRange, step: 1)
                        Text("\(Int(settings.fontSize)) pt").monospacedDigit().frame(width: 44, alignment: .trailing)
                    }
                }
                Picker("Decimals", selection: binding(\.decimals)) {
                    ForEach(Array(Settings.decimalsRange), id: \.self) { Text("\($0)").tag($0) }
                }
                .pickerStyle(.segmented)
                Toggle("Show BTC label", isOn: binding(\.showLabel))
                Toggle("Show change since the daily open", isOn: binding(\.showChange))
            }
            Section("Colors") {
                ColorPicker("Price", selection: color(\.textColor))
                ColorPicker("Up", selection: color(\.upColor))
                ColorPicker("Down", selection: color(\.downColor))
                LabeledContent("Background") {
                    Slider(value: binding(\.opacity), in: Settings.opacityRange)
                }
            }
            Section("Behavior") {
                Stepper("Refresh every \(settings.refreshSeconds) s", value: binding(\.refreshSeconds), in: Settings.refreshSecondsRange)
                Toggle("Show only while World of Warcraft is frontmost", isOn: binding(\.onlyWhenWoWActive))
                Toggle("Lock position", isOn: binding(\.locked))
                Button("Reset position") { edit { $0.position = nil } }
                    .disabled(settings.position == nil)
            }
        }
        .formStyle(.grouped)
        .frame(width: 400)
        .fixedSize(horizontal: false, vertical: true)
    }

    private func edit(_ change: (inout Settings) -> Void) {
        var settings = model.overlay.settings
        change(&settings)
        model.send(.settingsEdited(settings))
    }

    private func binding<Value>(_ key: WritableKeyPath<Settings, Value>) -> Binding<Value> {
        Binding(
            get: { model.overlay.settings[keyPath: key] },
            set: { value in edit { $0[keyPath: key] = value } }
        )
    }

    private func color(_ key: WritableKeyPath<Settings, RGBA>) -> Binding<Color> {
        Binding(
            get: { Color(nsColor: NSColor(model.overlay.settings[keyPath: key])) },
            set: { color in
                guard let srgb = NSColor(color).usingColorSpace(.sRGB) else { return }
                edit {
                    $0[keyPath: key] = RGBA(red: srgb.redComponent, green: srgb.greenComponent, blue: srgb.blueComponent, alpha: srgb.alphaComponent)
                }
            }
        )
    }
}
