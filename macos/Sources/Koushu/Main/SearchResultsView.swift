import KoushuCore
import SwiftUI

/// Search hits, grouped under the session they came from.
///
/// Grouping matters more than it looks: dictation produces many short
/// transcripts, so a flat list of eight hits is often three sessions, and
/// somebody scanning for "the conversation where I said that" wants the session.
struct SearchResultsView: View {
    @Bindable var app: AppModel
    @Bindable var browser: SessionBrowser

    private var groups: [(sessionID: String, title: String, dateKey: String, archived: Bool, hits: [SearchHit])] {
        guard let results = browser.results else { return [] }
        var order: [String] = []
        var buckets: [String: [SearchHit]] = [:]
        for hit in results.hits {
            if buckets[hit.sessionID] == nil { order.append(hit.sessionID) }
            buckets[hit.sessionID, default: []].append(hit)
        }
        return order.compactMap { id in
            guard let hits = buckets[id], let first = hits.first else { return nil }
            return (id, first.sessionTitle, first.dateKey, first.archived, hits)
        }
    }

    var body: some View {
        if let results = browser.results {
            if results.hits.isEmpty {
                empty
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 18) {
                        summary(results)
                        ForEach(groups, id: \.sessionID) { group in
                            section(group, terms: results.terms)
                        }
                    }
                    .frame(maxWidth: 760)
                    .frame(maxWidth: .infinity)
                    .padding(20)
                }
            }
        } else {
            Color.clear
        }
    }

    private func summary(_ results: SearchResponse) -> some View {
        Text(
            app.l(.searchSummary(
                matches: app.l(.matches(count: results.hits.count)),
                sessions: app.l(.sessionsCount(count: groups.count))
            )) + (results.truncated ? app.l(.searchTruncated) : "")
        )
        .font(.caption)
        .foregroundStyle(.secondary)
    }

    private func section(
        _ group: (sessionID: String, title: String, dateKey: String, archived: Bool, hits: [SearchHit]),
        terms: [String]
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Text(group.title)
                    .font(.caption.weight(.semibold))
                    .textCase(.uppercase)
                    .lineLimit(1)
                Text(Format.dayHeading(dateKey: group.dateKey, locale: app.locale))
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                if group.archived {
                    Text(app.l(.archivedTag))
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
            }
            ForEach(group.hits) { hit in
                Button {
                    Task { await browser.open(hit: hit) }
                } label: {
                    VStack(alignment: .leading, spacing: 6) {
                        highlighted(hit.snippet, terms: terms)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Text("\(Format.time(hit.createdAt, locale: app.locale)) · \(hit.language)")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                            .monospacedDigit()
                    }
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(.rect)
                }
                .buttonStyle(.plain)
                .glassEffect(.regular.interactive(), in: .rect(cornerRadius: 12, style: .continuous))
            }
        }
    }

    /// Marks each matched run inside the snippet.
    ///
    /// Built from the rendered text rather than from offsets handed over by the
    /// core: a Rust `char` index and a Swift `String.Index` disagree the moment
    /// an emoji appears, and a highlight one character off is worse than none.
    private func highlighted(_ text: String, terms: [String]) -> Text {
        let ranges = highlightRanges(in: text, terms: terms)
        guard !ranges.isEmpty else { return Text(text) }

        // One `AttributedString` rather than a chain of concatenated `Text`
        // values: concatenation is deprecated as of macOS 26, and an attributed
        // run is what the marking actually is.
        var attributed = AttributedString(text)
        for range in ranges {
            guard let lower = AttributedString.Index(range.lowerBound, within: attributed),
                  let upper = AttributedString.Index(range.upperBound, within: attributed)
            else { continue }
            attributed[lower..<upper].foregroundColor = .accentColor
            attributed[lower..<upper].inlinePresentationIntent = .stronglyEmphasized
        }
        return Text(attributed)
    }

    /// An empty result has three different explanations, and saying the wrong
    /// one sends the user to change something that was never the problem.
    private var empty: some View {
        VStack(spacing: 8) {
            Text(app.l(.searchEmptyTitle(query: browser.query.trimmingCharacters(in: .whitespaces))))
                .font(.title3.weight(.semibold))
                .multilineTextAlignment(.center)
            Text(explanation)
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 380)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(40)
    }

    private var explanation: String {
        if browser.filter.activeCount > 0 { return app.l(.searchEmptyFilters) }
        if browser.filter.archived == .active {
            return app.l(.searchEmptyArchived(scope: app.l(.scopeAll)))
        }
        return app.l(.searchEmptyHint)
    }
}
