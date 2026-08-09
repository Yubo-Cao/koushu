"use client";

import { Archive, ArchiveRestore, Search, SlidersHorizontal, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Button } from "@/components/Button";
import { useT } from "@/lib/i18n";
import { searchTranscripts, sessionFilterOptions } from "@/lib/tauri";
import type {
  ArchiveScope,
  FilterOptions,
  SearchResponse,
  SessionFilter,
  SessionInfo,
} from "@/lib/types";

/**
 * Search and filter state for the session sidebar.
 *
 * Kept in one hook so the page only has to hold a single value, and so the
 * query, the filters and the results can never drift out of step with each
 * other — the filters narrow both the session list and the search, and a stale
 * combination of the two would show hits from sessions the sidebar is hiding.
 */
export type SessionBrowser = ReturnType<typeof useSessionBrowser>;

const emptyFilter: SessionFilter = {};

export function useSessionBrowser() {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<SessionFilter>(emptyFilter);
  const [results, setResults] = useState<SearchResponse | null>(null);
  const [options, setOptions] = useState<FilterOptions | null>(null);
  // Every keystroke fires a query; only the newest answer may be rendered.
  // Local SQLite replies out of order rarely, but "rarely" here means the
  // results flicker back to a previous prefix, which looks like a bug.
  const generation = useRef(0);

  const refreshOptions = useCallback(() => {
    sessionFilterOptions()
      .then(setOptions)
      .catch(() => setOptions(null));
  }, []);

  useEffect(refreshOptions, [refreshOptions]);

  useEffect(() => {
    const term = query.trim();
    if (!term) {
      // Not a "no matches" state — there is nothing to match yet. Clearing the
      // results puts the plain session list back.
      generation.current += 1;
      setResults(null);
      return;
    }
    const mine = ++generation.current;
    // No loading state on purpose: this is a local index, it answers in
    // single-digit milliseconds, and a spinner that appears and vanishes on
    // every keystroke is noise, not feedback.
    searchTranscripts(query, filter)
      .then((response) => {
        if (generation.current === mine) setResults(response);
      })
      .catch(() => {
        if (generation.current === mine) setResults(null);
      });
  }, [query, filter]);

  const searching = query.trim().length > 0;
  const activeFilters = useMemo(() => countFilters(filter), [filter]);

  return {
    query,
    setQuery,
    filter,
    setFilter,
    results,
    options,
    refreshOptions,
    /** Whether the search results should stand in for the transcript view. */
    searching,
    /** How many filters are narrowing the view, for the disclosure badge. */
    activeFilters,
    clear: useCallback(() => setQuery(""), []),
    resetFilters: useCallback(() => setFilter(emptyFilter), []),
  };
}

function countFilters(filter: SessionFilter): number {
  return [
    filter.language,
    filter.model,
    filter.from,
    filter.to,
    filter.archived && filter.archived !== "active" ? filter.archived : "",
  ].filter((value) => Boolean(value)).length;
}

type SessionSearchProps = {
  browser: SessionBrowser;
  activeSession: SessionInfo | null;
  onArchive: (session: SessionInfo, archived: boolean) => void;
};

/**
 * The sidebar's search box, its filters and the archive action.
 *
 * The filters hide behind a disclosure rather than sitting open: five controls
 * permanently above the session list would cost more vertical space than the
 * list they are filtering. The count on the toggle is what keeps a collapsed
 * filter from being a filter you forgot you set.
 */
export function SessionSearch({ browser, activeSession, onArchive }: SessionSearchProps) {
  const t = useT();
  const [showFilters, setShowFilters] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const { query, setQuery, filter, setFilter, options, activeFilters } = browser;

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key.toLowerCase() === "f" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // A filter that is set but hidden is a trap, so opening is automatic whenever
  // one is active and the panel is closed.
  useEffect(() => {
    if (activeFilters > 0) setShowFilters(true);
  }, [activeFilters]);

  function patch(change: Partial<SessionFilter>) {
    setFilter({ ...filter, ...change });
  }

  const archived = Boolean(activeSession?.archived_at);

  return (
    <div className="shrink-0 space-y-1.5 px-3 pb-2">
      <div className="relative">
        <Search
          size={13}
          aria-hidden
          className="pointer-events-none absolute top-1/2 left-2.5 -translate-y-1/2 text-faint"
        />
        <input
          ref={inputRef}
          type="search"
          value={query}
          spellCheck={false}
          placeholder={t("search.placeholder")}
          aria-label={t("search.placeholder")}
          className="field w-full pl-7"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && query) {
              event.stopPropagation();
              setQuery("");
            }
          }}
        />
        {query ? (
          <button
            type="button"
            aria-label={t("search.clear")}
            className="press absolute top-1/2 right-1.5 -translate-y-1/2 rounded p-1 text-faint hover:text-ink"
            onClick={() => {
              setQuery("");
              inputRef.current?.focus();
            }}
          >
            <X size={13} />
          </button>
        ) : null}
      </div>

      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="sm"
          aria-expanded={showFilters}
          icon={<SlidersHorizontal size={12} />}
          onClick={() => setShowFilters((open) => !open)}
        >
          <span className="t-micro">
            {t("search.filters")}
            {activeFilters > 0 ? ` · ${activeFilters}` : ""}
          </span>
        </Button>
        <div className="flex-1" />
        {activeSession ? (
          <Button
            variant="ghost"
            size="sm"
            // The tooltip carries the reassurance. "Archive" next to a session
            // list reads as "delete" to anyone who has used email, and this
            // only ever sets a date column.
            title={
              archived
                ? t("search.restoreTitle", { title: activeSession.title })
                : t("search.archiveTitle", { title: activeSession.title })
            }
            icon={archived ? <ArchiveRestore size={12} /> : <Archive size={12} />}
            onClick={() => onArchive(activeSession, !archived)}
          >
            <span className="t-micro">{archived ? t("search.restore") : t("search.archive")}</span>
          </Button>
        ) : null}
      </div>

      {showFilters ? (
        <div className="space-y-1.5 pt-0.5">
          <div className="flex gap-1.5">
            <select
              aria-label={t("search.filterLanguage")}
              className="field ctl-sm min-w-0 flex-1"
              value={filter.language ?? ""}
              onChange={(event) => patch({ language: event.target.value })}
            >
              <option value="">{t("search.anyLanguage")}</option>
              {(options?.languages ?? []).map((language) => (
                <option key={language} value={language}>
                  {language}
                </option>
              ))}
            </select>
            <select
              aria-label={t("search.filterModel")}
              className="field ctl-sm min-w-0 flex-1"
              value={filter.model ?? ""}
              onChange={(event) => patch({ model: event.target.value })}
            >
              <option value="">{t("search.anyModel")}</option>
              {(options?.models ?? []).map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </select>
          </div>
          <div className="flex items-center gap-1.5">
            <input
              type="date"
              aria-label={t("search.fromDate")}
              className="field ctl-sm min-w-0 flex-1"
              value={filter.from ?? ""}
              min={options?.earliestDate ?? undefined}
              max={filter.to || (options?.latestDate ?? undefined)}
              onChange={(event) => patch({ from: event.target.value })}
            />
            <span className="t-micro text-meta text-faint">{t("search.dateSeparator")}</span>
            <input
              type="date"
              aria-label={t("search.toDate")}
              className="field ctl-sm min-w-0 flex-1"
              value={filter.to ?? ""}
              min={filter.from || (options?.earliestDate ?? undefined)}
              max={options?.latestDate ?? undefined}
              onChange={(event) => patch({ to: event.target.value })}
            />
          </div>
          <div className="flex items-center gap-1.5">
            <select
              aria-label={t("search.archiveScope")}
              className="field ctl-sm min-w-0 flex-1"
              value={filter.archived ?? "active"}
              onChange={(event) =>
                patch({ archived: event.target.value as ArchiveScope })
              }
            >
              <option value="active">{t("search.scopeActive")}</option>
              <option value="archived">
                {t("search.scopeArchived")}
                {options ? ` (${options.archivedCount})` : ""}
              </option>
              <option value="all">{t("search.scopeAll")}</option>
            </select>
            {activeFilters > 0 ? (
              <Button variant="ghost" size="sm" onClick={browser.resetFilters}>
                <span className="t-micro">{t("search.reset")}</span>
              </Button>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}

/**
 * Splits `text` so that each occurrence of any search term can be marked.
 *
 * Done on the rendered snippet rather than by returning offsets from Rust: a
 * Rust `char` index and a JavaScript string index disagree the moment an emoji
 * appears, and a highlight one character off is worse than none.
 */
export function highlight(text: string, terms: string[]): ReactNode {
  const usable = terms.filter((term) => term.trim().length > 0);
  if (usable.length === 0) return text;

  const pattern = new RegExp(
    `(${usable.map(escapeRegExp).sort((a, b) => b.length - a.length).join("|")})`,
    "gi",
  );
  return text.split(pattern).map((part, index) =>
    index % 2 === 1 ? (
      <mark key={index} className="rounded-[3px] bg-accent/22 px-px text-ink">
        {part}
      </mark>
    ) : (
      part
    ),
  );
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

type SearchResultsProps = {
  browser: SessionBrowser;
  onOpen: (sessionId: string, transcriptId: string) => void;
};

/**
 * Search hits, grouped under the session they came from.
 *
 * Grouping matters more than it looks: dictation produces many short
 * transcripts, so a flat list of eight hits is often three sessions, and a user
 * scanning for "the conversation where I said that" wants the session.
 */
export function SearchResults({ browser, onOpen }: SearchResultsProps) {
  const t = useT();
  const { results, query, filter, activeFilters } = browser;
  const groups = useMemo(() => groupBySession(results), [results]);

  if (!results) return null;

  if (results.hits.length === 0) {
    return (
      <div className="flex h-full items-center justify-center px-5 text-center">
        <div className="max-w-sm">
          <p className="t-title text-title font-semibold">
            {t("search.empty.title", { query: query.trim() })}
          </p>
          <p className="t-body mt-1.5 text-ui text-smoke">
            {activeFilters > 0
              ? t("search.empty.filters")
              : filter.archived === "active"
                ? t("search.empty.archived", { scope: t("search.scopeAll") })
                : t("search.empty.hint")}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl space-y-4 px-5 py-5">
      <p className="t-micro text-meta text-smoke">
        {t("search.summary", {
          matches: t(results.hits.length === 1 ? "search.matchesOne" : "search.matches", {
            count: results.hits.length,
          }),
          sessions: t(groups.length === 1 ? "search.sessionsOne" : "search.sessions", {
            count: groups.length,
          }),
        })}
        {results.truncated ? t("search.truncated") : ""}
      </p>

      {groups.map((group) => (
        <section key={group.sessionId}>
          <p className="t-micro mb-1 flex items-center gap-1.5 text-meta font-semibold tracking-wider uppercase text-faint">
            <span className="truncate">{group.title}</span>
            <span className="shrink-0 font-normal normal-case tracking-normal">
              {group.dateKey}
            </span>
            {group.archived ? (
              <span className="shrink-0 font-normal normal-case tracking-normal">
                {t("search.archivedTag")}
              </span>
            ) : null}
          </p>
          <div className="space-y-1.5">
            {group.hits.map((hit) => (
              <button
                key={hit.transcriptId}
                type="button"
                className="press glass rim block w-full rounded-lg p-3 text-left hover:bg-fill"
                onClick={() => onOpen(hit.sessionId, hit.transcriptId)}
              >
                <span className="t-body block text-ui text-ink-2">
                  {highlight(hit.snippet, results.terms)}
                </span>
                <span className="tnum t-micro mt-1.5 block text-meta text-faint">
                  {new Date(hit.createdAt).toLocaleTimeString([], {
                    hour: "2-digit",
                    minute: "2-digit",
                  })}{" "}
                  · {hit.language}
                </span>
              </button>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

type HitGroup = {
  sessionId: string;
  title: string;
  dateKey: string;
  archived: boolean;
  hits: SearchResponse["hits"];
};

function groupBySession(results: SearchResponse | null): HitGroup[] {
  if (!results) return [];
  const groups = new Map<string, HitGroup>();
  for (const hit of results.hits) {
    const group = groups.get(hit.sessionId) ?? {
      sessionId: hit.sessionId,
      title: hit.sessionTitle,
      dateKey: hit.dateKey,
      archived: hit.archived,
      hits: [],
    };
    group.hits.push(hit);
    groups.set(hit.sessionId, group);
  }
  return [...groups.values()];
}
