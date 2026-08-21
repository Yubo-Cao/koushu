import KoushuCore
import SwiftUI

/// The control deck: what the next recording does, and proof that the
/// microphone is listening.
///
/// Reading left to right in the order the user acts on them — the model, the
/// language, the microphone, then the meter that says it is working. The Talk
/// button that used to lead this row is gone: this app is driven by holding a
/// key, and a button that starts a recording in a window the user is not looking
/// at was answering a question nobody asked. What it is replaced by is the
/// shortcut itself, stated, so the window says how to record rather than
/// offering a second way.
struct ControlDeck: View {
    @Bindable var app: AppModel
    @Bindable var browser: SessionBrowser

    var body: some View {
        HStack(spacing: 10) {
            recordingIndicator

            Divider().frame(height: 18)

            Picker(selection: $app.defaultModelID) {
                ForEach(app.models) { model in
                    Text(model.name.replacingOccurrences(
                        of: #"\s*[(（].*[)）]$"#,
                        with: "",
                        options: .regularExpression
                    )).tag(model.id)
                }
            } label: {
                Label(app.l(.deckModel), systemImage: "shippingbox")
            }
            .labelsHidden()
            .frame(minWidth: 130, maxWidth: 200)
            .help(app.l(.deckModel))
            .onChange(of: app.defaultModelID) { _, value in
                Task {
                    await app.save(SettingKey.defaultModel, value)
                    await app.save(SettingKey.defaultRuntime, app.runtime)
                }
            }

            Picker(selection: $app.defaultLanguage) {
                ForEach(transcriptionLanguages, id: \.self) { Text($0).tag($0) }
            } label: {
                Label(app.l(.deckLanguage), systemImage: "character.bubble")
            }
            .labelsHidden()
            .frame(minWidth: 96, maxWidth: 140)
            .help(app.l(.deckLanguage))
            .onChange(of: app.defaultLanguage) { _, value in
                Task { await app.save(SettingKey.defaultLanguage, value) }
            }

            microphonePicker

            Spacer(minLength: 8)

            meter
        }
        .controlSize(.regular)
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(.bar)
    }

    // MARK: Recording state

    /// What the app is doing, and — when it is doing nothing — how to make it
    /// do something.
    @ViewBuilder
    private var recordingIndicator: some View {
        switch app.activity {
        case .recording:
            Label(app.l(.statusListening), systemImage: "mic.fill")
                .foregroundStyle(.red)
                .font(.callout.weight(.medium))
        case .transcribing:
            Label(app.l(.statusTranscribing), systemImage: "waveform")
                .font(.callout.weight(.medium))
        case .idle:
            HStack(spacing: 6) {
                Image(systemName: app.hotkeyArmed ? "keyboard" : "keyboard.badge.ellipsis")
                    .foregroundStyle(app.hotkeyArmed ? AnyShapeStyle(.secondary) : AnyShapeStyle(.red))
                Text(app.hotkeyArmed
                    ? app.chord.display(spaceLabel: app.l(.hotkeyKeySpace))
                    : app.l(.hotkeyNotBound))
                    .font(.callout.weight(.medium))
                    .foregroundStyle(app.hotkeyArmed ? AnyShapeStyle(.primary) : AnyShapeStyle(.red))
            }
            .help(app.hotkeyArmed ? app.l(.hotkeyDesc) : app.l(.hotkeyNeedsAccessibility))
        }
    }

    // MARK: Microphone

    private var microphonePicker: some View {
        Picker(selection: $app.audioInputID) {
            // The resolved device name lives in the tooltip, not in the closed
            // control: a Core Audio aggregate name runs to fifty characters and
            // no control that fits in a bar will ever show one.
            Text(app.l(.deckMicrophoneNone)).tag("")
            ForEach(app.audioInputs) { input in
                Text(input.name).tag(input.id)
            }
        } label: {
            Label(app.l(.deckMicrophone), systemImage: "mic")
        }
        .labelsHidden()
        .frame(minWidth: 120, maxWidth: 200)
        .disabled(app.activity != .idle)
        .help(defaultInput.map { app.l(.deckMicrophoneDefault(name: $0.name)) } ?? app.l(.deckMicrophone))
        .onChange(of: app.audioInputID) { _, value in
            Task { await app.save(SettingKey.audioInput, value) }
        }
    }

    private var defaultInput: AudioInputInfo? {
        app.audioInputs.first { $0.isDefault }
    }

    // MARK: Meter

    /// A meter, not a card.
    ///
    /// Stacking a label above the bar made this the tallest thing in the deck
    /// and forced the whole strip to its height; laid out in one line it drops
    /// onto the same height as the pickers beside it, and the reading stays
    /// exactly as legible.
    @ViewBuilder
    private var meter: some View {
        if app.audioInputs.isEmpty {
            Text(app.l(.deckNoMicrophone))
                .font(.callout)
                .foregroundStyle(.red)
                .lineLimit(1)
        } else {
            HStack(spacing: 8) {
                Text(app.l(.deckInput))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Capsule()
                    .fill(.quaternary)
                    .frame(width: 90, height: 3)
                    .overlay(alignment: .leading) {
                        Capsule()
                            .fill(.tint)
                            .frame(width: 90 * (app.activity == .recording ? app.level : 0), height: 3)
                    }
                Text(app.activity == .recording ? "\(Int(app.level * 100))%" : app.l(.deckIdle))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .frame(width: 42, alignment: .trailing)
            }
        }
    }
}
