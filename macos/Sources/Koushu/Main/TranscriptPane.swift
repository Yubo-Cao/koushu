import KoushuCore
import SwiftUI

/// The transcripts of the selected session, newest at the bottom.
struct TranscriptPane: View {
    @Bindable var app: AppModel
    @Bindable var browser: SessionBrowser

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                if browser.transcripts.isEmpty && app.partial.isEmpty {
                    empty
                } else {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(browser.transcripts) { transcript in
                            TranscriptCard(app: app, browser: browser, transcript: transcript)
                                .id(transcript.id)
                        }
                        if !app.partial.isEmpty {
                            livePartial
                        }
                    }
                    .frame(maxWidth: 760)
                    .frame(maxWidth: .infinity)
                    .padding(20)
                }
            }
            // A search hit opens its session and then has to find the one
            // transcript in it. The scroll cannot happen when the hit is clicked
            // — the row does not exist yet — so it waits until the list it lives
            // in has actually arrived.
            .onChange(of: browser.transcripts.map(\.id)) { _, ids in
                guard let target = browser.pendingScroll, ids.contains(target) else { return }
                browser.pendingScroll = nil
                withAnimation(Motion.content) {
                    proxy.scrollTo(target, anchor: .center)
                }
            }
        }
    }

    private var empty: some View {
        VStack(spacing: 10) {
            Image(systemName: "mic")
                .font(.system(size: 26))
                .foregroundStyle(.tint)
                .frame(width: 56, height: 56)
                .glassEffect(.regular, in: .rect(cornerRadius: 19, style: .continuous))
            Text(app.l(.emptyTitle))
                .font(.title3.weight(.semibold))
            Text(app.l(.emptyBody))
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 340)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(40)
    }

    /// The still-changing decode. Marked as a preview because it is one: segment
    /// boundaries cut mid-sentence, and the authoritative text is the one
    /// produced by decoding the whole recording at once.
    private var livePartial: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(app.l(.transcriptLivePartial), systemImage: "waveform")
                .font(.caption.weight(.medium))
                .foregroundStyle(.red)
            Text(app.partial)
                .textSelection(.enabled)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .glassEffect(.regular, in: .rect(cornerRadius: 12, style: .continuous))
    }
}

/// One saved transcript, with the formatted version below it when there is one.
struct TranscriptCard: View {
    @Bindable var app: AppModel
    @Bindable var browser: SessionBrowser
    let transcript: TranscriptInfo

    private var isFormatting: Bool { browser.formatting[transcript.id] != nil }
    private var streamed: String? { browser.formatting[transcript.id] }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            header
            Text(transcript.text)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)

            if isFormatting || transcript.formattedText != nil {
                Divider()
                formatted
            }

            if let error = browser.formatErrors[transcript.id] {
                Text(error)
                    .font(.callout)
                    .foregroundStyle(.red)
            }
        }
        .padding(14)
        .glassEffect(.regular, in: .rect(cornerRadius: 12, style: .continuous))
    }

    private var header: some View {
        HStack(spacing: 8) {
            Text("\(Format.time(transcript.createdAt, locale: app.locale)) · \(transcript.language)")
                .font(.caption)
                .foregroundStyle(.secondary)
                .monospacedDigit()
            Spacer()

            Button {
                browser.format(transcript, preset: app.llm.preset)
            } label: {
                Label(formatLabel, systemImage: "wand.and.stars")
            }
            // Formatting is only offered once there is somewhere to send it.
            // Offering it without an endpoint means a button whose only possible
            // outcome is an error.
            .disabled(!app.llm.isConfigured || isFormatting)
            .help(app.llm.isConfigured
                ? app.l(.transcriptFormatTitle)
                : app.l(.transcriptFormatDisabled))

            Button {
                TextInjector.copyToPasteboard(transcript.formattedText ?? transcript.text)
            } label: {
                Label(app.l(.copy), systemImage: "document.on.document")
                    .labelStyle(.iconOnly)
            }
            .help(app.l(.copy))
        }
        .buttonStyle(.borderless)
        .controlSize(.small)
    }

    private var formatLabel: String {
        if isFormatting { return app.l(.transcriptFormatting) }
        return transcript.formattedText == nil ? app.l(.transcriptFormat) : app.l(.transcriptRedo)
    }

    private var formatted: some View {
        VStack(alignment: .leading, spacing: 6) {
            Label(
                isFormatting
                    ? app.l(.transcriptFormatting)
                    : app.l(.transcriptFormatted(preset: transcript.formattedPreset ?? "typeset")),
                systemImage: "wand.and.stars"
            )
            .font(.caption.weight(.medium))
            .foregroundStyle(.tint)

            // The stream while it is running, the stored text once it is not.
            // The raw transcript above is never overwritten by either.
            Text(streamed ?? transcript.formattedText ?? "")
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
