import Foundation
import Testing

@testable import KoushuCore

// What is worth testing here is the logic that has no pixels: the chord
// grammar, the search the sidebar depends on, and the byte formatting that has
// to agree with the download page it is quoted from. The views are checked by
// looking at them; these cannot be.

@Suite("Chord")
struct ChordTests {
    @Test("The stored spelling round-trips through the XDG form")
    func roundTrip() {
        let chord = Chord(modifiers: [.option, .control], key: "space")
        // Canonical order, whatever order the modifiers went down in — the same
        // chord must not produce two different rows in the settings table.
        #expect(chord.stored == "CTRL+ALT+space")
        #expect(Chord(stored: "CTRL+ALT+space") == chord)
    }

    @Test("A chord the Tauri build wrote is readable here")
    func parsesLinuxSpelling() {
        let chord = Chord(stored: "CTRL+SHIFT+k")
        #expect(chord?.key == "k")
        #expect(chord?.modifiers == [.control, .shift])
    }

    @Test("Unbindable keys are refused rather than stored")
    func refusesUnknownKeys() {
        #expect(Chord(stored: "CTRL+Tab") == nil)
        #expect(Chord(stored: "CTRL+Escape") == nil)
        #expect(Chord.validate(modifiers: [.control], key: "Return") == .unsupportedKey)
    }

    @Test("A bare key is refused, because a tap would take it system-wide")
    func requiresAModifier() {
        #expect(Chord.validate(modifiers: [], key: "space") == .needsModifier)
        #expect(Chord.validate(modifiers: [.control], key: "space") == nil)
    }

    @Test("Every supported key name maps to a code and back")
    func keyCodesRoundTrip() {
        for name in ["space", "a", "z", "0", "9", "F1", "F20"] {
            let code = Chord.keyCode(for: name)
            #expect(code != nil, "no key code for \(name)")
            #expect(Chord.keyName(for: code!)?.lowercased() == name.lowercased())
        }
    }

    @Test("Display uses Apple's glyphs in Apple's order")
    func display() {
        let chord = Chord(modifiers: [.command, .control], key: "space")
        #expect(chord.display(spaceLabel: "Space") == "⌃⌘Space")
    }
}

@Suite("Search")
struct SearchTests {
    @Test("A hit carries a window of text around the match")
    func snippets() async throws {
        let database = StubDatabase()
        let store = StubSessionStore(database: database)
        let response = try await store.search(query: "placeholder", filter: .none, limit: 20)
        #expect(!response.hits.isEmpty)
        #expect(response.hits.allSatisfy { $0.snippet.lowercased().contains("placeholder") })
    }

    @Test("Archived sessions stay out of the default scope, and come back on request")
    func archiveScope() async throws {
        let database = StubDatabase()
        let store = StubSessionStore(database: database)

        let active = try await store.search(query: "focus", filter: .none, limit: 20)
        let everything = try await store.search(
            query: "focus",
            filter: SessionFilter(archived: .all),
            limit: 20
        )
        // The seeded archived session is the only one containing this word, so
        // the default scope must miss it and `.all` must find it. This is the
        // rule that makes "no results" explainable rather than mysterious.
        #expect(active.hits.isEmpty)
        #expect(!everything.hits.isEmpty)
    }

    @Test("Short terms are reported as a scan rather than an index lookup")
    func mode() async throws {
        let store = StubSessionStore(database: StubDatabase())
        let long = try await store.search(query: "placeholder", filter: .none, limit: 20)
        let short = try await store.search(query: "是", filter: .none, limit: 20)
        #expect(long.mode == .fts)
        #expect(short.mode == .substring)
    }

    @Test("Archiving hides a session from the list without deleting it")
    func archiving() async throws {
        let store = StubSessionStore(database: StubDatabase())
        let before = try await store.sessions(limit: 50, filter: .none)
        let target = try #require(before.first)

        try await store.setArchived(sessionID: target.id, archived: true)
        let after = try await store.sessions(limit: 50, filter: .none)
        let all = try await store.sessions(limit: 50, filter: SessionFilter(archived: .all))

        #expect(!after.contains { $0.id == target.id })
        #expect(all.contains { $0.id == target.id })
    }
}

@Suite("Highlighting")
struct HighlightTests {
    @Test("Overlapping terms mark the longest match once")
    func longestWins() {
        let text = "transcription and transcript"
        let ranges = highlightRanges(in: text, terms: ["transcript", "transcription"])
        #expect(ranges.count == 2)
        #expect(String(text[ranges[0]]) == "transcription")
        #expect(String(text[ranges[1]]) == "transcript")
    }

    @Test("No terms means no marks, not a crash")
    func empty() {
        #expect(highlightRanges(in: "anything", terms: []).isEmpty)
        #expect(highlightRanges(in: "anything", terms: ["   "]).isEmpty)
    }
}

@Suite("Formatting")
struct FormattingTests {
    @Test("Byte sizes match the units the download page quotes")
    func bytes() {
        #expect(Format.bytes(nil) == "–")
        #expect(Format.bytes(0) == "–")
        #expect(Format.bytes(940_572_620) == "897 MB")
        #expect(Format.bytes(2_147_483_648) == "2.0 GB")
    }

    @Test("Elapsed time is minutes and seconds, never negative")
    func elapsed() {
        #expect(Format.elapsed(0) == "0:00")
        #expect(Format.elapsed(65) == "1:05")
        #expect(Format.elapsed(-3) == "0:00")
    }
}

@Suite("Messages")
struct MessageTests {
    @Test("Both locales render every message non-empty")
    func bothLocalesComplete() {
        // Exhaustiveness is a compile-time property here — a missing case in
        // either switch does not build. What a test can still add is that
        // nothing was filled in with an empty string to satisfy the compiler.
        let samples: [Msg] = [
            .appName, .deckTalk, .searchPlaceholder, .settingsTitle, .trayIdle,
            .hotkeyNeedsAccessibility, .stubCoreNotice,
            .downloadDownloaded(size: "1 MB"),
            .matches(count: 2), .sessionsCount(count: 1),
            .trialUsed(minutes: 6), .searchEmptyTitle(query: "x"),
        ]
        for message in samples {
            for locale in UILocale.allCases {
                #expect(!message.text(in: locale).isEmpty)
            }
        }
    }

    @Test("English composes counts rather than pluralising with an s")
    func counts() {
        #expect(Msg.matches(count: 1).text(in: .en) == "1 match")
        #expect(Msg.matches(count: 4).text(in: .en) == "4 matches")
        #expect(Msg.sessionsCount(count: 1).text(in: .en) == "1 session")
    }
}
