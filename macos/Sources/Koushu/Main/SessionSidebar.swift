import KoushuCore
import SwiftUI

/// Sessions grouped by day, with the filters that narrow them.
///
/// One line per session, not two. In the Tauri build the language started as a
/// full second line under every title, which made a row 57px tall and put eight
/// sessions on a screen that comfortably holds fifteen. It is secondary
/// information, so it sits where secondary information goes — trailing, quiet,
/// on the same line.
struct SessionSidebar: View {
    @Bindable var app: AppModel
    @Bindable var browser: SessionBrowser

    /// Filters hide behind a disclosure rather than sitting open: five controls
    /// permanently above the session list would cost more vertical space than
    /// the list they are filtering.
    @State private var showFilters = false

    var body: some View {
        List(selection: selection) {
            filterSection

            ForEach(browser.grouped, id: \.dateKey) { group in
                Section(Format.dayHeading(dateKey: group.dateKey, locale: app.locale)) {
                    ForEach(group.sessions) { session in
                        row(session)
                            .tag(session.id)
                    }
                }
            }

            if browser.sessions.isEmpty {
                // An empty sidebar under an active filter looks identical to an
                // empty sidebar in a brand-new install.
                Text(emptyMessage)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .listRowSeparator(.hidden)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .top, spacing: 0) {
            Button {
                Task { await newSession() }
            } label: {
                Label(app.l(.newSession), systemImage: "plus")
                    .frame(maxWidth: .infinity)
            }
            .controlSize(.large)
            .buttonStyle(.borderedProminent)
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
        }
        // Opening is automatic whenever a filter is set and the panel is closed:
        // a filter that is active but hidden is a trap.
        .onChange(of: browser.filter.activeCount) { _, count in
            if count > 0 { showFilters = true }
        }
    }

    private var selection: Binding<String?> {
        Binding(
            get: { browser.activeSessionID },
            set: { id in Task { await browser.select(id) } }
        )
    }

    private var emptyMessage: String {
        if browser.filter.activeCount > 0 { return app.l(.sidebarNoMatches) }
        if browser.filter.archived == .archived { return app.l(.sidebarNoArchived) }
        return app.l(.sidebarNoSessions)
    }

    // MARK: Rows

    private func row(_ session: SessionInfo) -> some View {
        HStack(spacing: 6) {
            Text(session.title)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 4)
            // Without this, the "Everything" archive scope shows archived and
            // active sessions in one indistinguishable list.
            if session.isArchived {
                Image(systemName: "archivebox")
                    .imageScale(.small)
                    .foregroundStyle(.tertiary)
                    .accessibilityLabel(app.l(.scopeArchived))
            }
            Text(session.language)
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .contextMenu {
            Button(session.isArchived ? app.l(.restore) : app.l(.archive)) {
                Task { await toggleArchive(session) }
            }
        }
        .help(session.isArchived
            ? app.l(.restoreTitle(title: session.title))
            : app.l(.archiveTitle(title: session.title)))
    }

    // MARK: Filters

    @ViewBuilder
    private var filterSection: some View {
        DisclosureGroup(isExpanded: $showFilters) {
            Picker(app.l(.filterLanguage), selection: languageBinding) {
                Text(app.l(.anyLanguage)).tag("")
                ForEach(browser.options.languages, id: \.self) { Text($0).tag($0) }
            }
            Picker(app.l(.filterModel), selection: modelBinding) {
                Text(app.l(.anyModel)).tag("")
                ForEach(browser.options.models, id: \.self) { Text($0).tag($0) }
            }
            Picker(app.l(.archiveScope), selection: scopeBinding) {
                ForEach(ArchiveScope.allCases, id: \.self) { scope in
                    Text(scopeLabel(scope)).tag(scope)
                }
            }
            HStack(spacing: 6) {
                DatePicker(
                    app.l(.fromDate),
                    selection: dateBinding(\.from, fallback: earliest),
                    displayedComponents: .date
                )
                .labelsHidden()
                Text(app.l(.dateSeparator))
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                DatePicker(
                    app.l(.toDate),
                    selection: dateBinding(\.to, fallback: Date()),
                    displayedComponents: .date
                )
                .labelsHidden()
            }

            if browser.filter.activeCount > 0 {
                Button(app.l(.searchReset)) { browser.resetFilters() }
            }
        } label: {
            Label {
                Text(browser.filter.activeCount > 0
                    ? "\(app.l(.searchFilters)) · \(browser.filter.activeCount)"
                    : app.l(.searchFilters))
            } icon: {
                Image(systemName: "line.3.horizontal.decrease")
            }
        }
        .pickerStyle(.menu)
        .controlSize(.small)
    }

    /// The archived scope carries the count, so choosing it is a decision made
    /// with the number in view rather than after the list turns out to be empty.
    private func scopeLabel(_ scope: ArchiveScope) -> String {
        let base = app.l(scope.message)
        guard scope == .archived, browser.options.archivedCount > 0 else { return base }
        return "\(base) (\(browser.options.archivedCount))"
    }

    private var earliest: Date {
        browser.options.earliestDate.flatMap(Format.date(fromKey:)) ?? Date()
    }

    private var languageBinding: Binding<String> {
        Binding(
            get: { browser.filter.language ?? "" },
            set: { browser.filter.language = $0.isEmpty ? nil : $0 }
        )
    }

    private var modelBinding: Binding<String> {
        Binding(
            get: { browser.filter.model ?? "" },
            set: { browser.filter.model = $0.isEmpty ? nil : $0 }
        )
    }

    private var scopeBinding: Binding<ArchiveScope> {
        Binding(
            get: { browser.filter.archived },
            set: { browser.filter.archived = $0 }
        )
    }

    /// A `DatePicker` deals in `Date`; the filter deals in the `YYYY-MM-DD` keys
    /// the store compares against. Converting here rather than in the filter
    /// keeps the stored form identical to the one the Tauri build writes.
    private func dateBinding(
        _ path: WritableKeyPath<SessionFilter, String?>,
        fallback: Date
    ) -> Binding<Date> {
        Binding(
            get: { browser.filter[keyPath: path].flatMap(Format.date(fromKey:)) ?? fallback },
            set: { browser.filter[keyPath: path] = Format.dateKey($0) }
        )
    }

    // MARK: Actions

    private func newSession() async {
        let time = Format.time(Date(), locale: app.locale)
        _ = await browser.createSession(
            title: app.l(.sessionTitle(time: time)),
            model: app.defaultModelID,
            language: app.defaultLanguage,
            runtime: app.runtime
        )
    }

    private func toggleArchive(_ session: SessionInfo) async {
        let archived = !session.isArchived
        await browser.setArchived(session, archived)
        app.coreMessage = archived
            ? app.l(.statusArchived(title: session.title))
            : app.l(.statusRestored(title: session.title))
    }
}
