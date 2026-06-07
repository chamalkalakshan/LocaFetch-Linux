import { useEffect, useRef, useState, useCallback } from "react";
import { FileResult, IndexStatus, searchFiles, getIndexStatus, getFileCount, openFile, hideWindow } from "../lib/api";
import ResultItem from "./ResultItem";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

interface Props {
  onSettings: () => void;
  filter: string;
  onFilterChange: (f: string) => void;
}

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return debounced;
}

const FILTERS = [
  { id: "all",     label: "All"       },
  { id: "folder",  label: "📁 Folders" },
  { id: "video",   label: "🎬 Video"  },
  { id: "audio",   label: "🎵 Audio"  },
  { id: "image",   label: "🖼️ Image"  },
  { id: "doc",     label: "📄 Docs"   },
  { id: "archive", label: "📦 Archive"},
];

export default function SearchView({ onSettings, filter, onFilterChange }: Props) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<FileResult[]>([]);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [status, setStatus] = useState<IndexStatus | null>(null);
  const [fileCount, setFileCount] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const debouncedQuery = useDebounce(query, 80);

  const COLLAPSED_H = 68;
  const MAX_VISIBLE = 7;
  const FILTER_BAR = 36;

  function calcHeight(q: string, resultCount: number): number {
    if (!q.trim()) return COLLAPSED_H;
    const SEARCH = 68;
    const SEP = 1;
    const EMPTY = 72;
    const ROW = 52;
    const PAD = 12;
    const STATUS = 26;
    if (resultCount === 0) return SEARCH + SEP + FILTER_BAR + EMPTY;
    const rows = Math.min(resultCount, MAX_VISIBLE);
    return SEARCH + SEP + FILTER_BAR + PAD + rows * ROW + STATUS;
  }

  // Clear and focus when window is shown (Ctrl+Space); just focus when regaining focus
  useEffect(() => {
    inputRef.current?.focus();
    const win = getCurrentWindow();
    let unlistenShow: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;
    win.listen("search-shown", () => {
      setQuery("");
      setResults([]);
      setTimeout(() => inputRef.current?.focus(), 30);
    }).then((fn) => { unlistenShow = fn; });
    win.listen("tauri://focus", () => {
      setTimeout(() => inputRef.current?.focus(), 30);
    }).then((fn) => { unlistenFocus = fn; });
    return () => { unlistenShow?.(); unlistenFocus?.(); };
  }, []);

  // ESC closes from anywhere on the page
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setQuery("");
        setResults([]);
        hideWindow();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  // Resize window to fit content exactly
  useEffect(() => {
    getCurrentWindow()
      .setSize(new LogicalSize(680, calcHeight(query, results.length)))
      .catch(() => {});
  }, [query, results]);

  // Poll index status every 2s while indexing
  useEffect(() => {
    const poll = async () => {
      const s = await getIndexStatus().catch(() => null);
      if (s) setStatus(s);
      const c = await getFileCount().catch(() => 0);
      setFileCount(c);
    };
    poll();
    const id = setInterval(poll, 2000);
    return () => clearInterval(id);
  }, []);

  // Search on debounced query or filter change
  useEffect(() => {
    if (!debouncedQuery.trim()) {
      setResults([]);
      setSelectedIdx(0);
      return;
    }
    searchFiles(debouncedQuery, filter === "all" ? "" : filter)
      .then((r) => {
        setResults(r);
        setSelectedIdx(0);
      })
      .catch(() => setResults([]));
  }, [debouncedQuery, filter]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case "Escape":
          setQuery("");
          setResults([]);
          hideWindow();
          break;
        case "ArrowDown":
          e.preventDefault();
          setSelectedIdx((i) => Math.min(i + 1, results.length - 1));
          break;
        case "ArrowUp":
          e.preventDefault();
          setSelectedIdx((i) => Math.max(i - 1, 0));
          break;
        case "Enter":
          if (results[selectedIdx]) openFile(results[selectedIdx].path);
          break;
        case ",":
          if (e.ctrlKey) {
            e.preventDefault();
            onSettings();
          }
          break;
      }
    },
    [results, selectedIdx, onSettings]
  );

  const isIndexing = status?.is_indexing ?? false;

  return (
    <>
      {/* Search bar */}
      <div className="search-bar">
        <svg className="search-icon" viewBox="0 0 20 20" fill="currentColor">
          <path
            fillRule="evenodd"
            d="M9 3a6 6 0 100 12A6 6 0 009 3zM1 9a8 8 0 1114.32 4.906l3.387 3.387a1 1 0 01-1.414 1.414l-3.387-3.387A8 8 0 011 9z"
            clipRule="evenodd"
          />
        </svg>
        <input
          ref={inputRef}
          className="search-input"
          placeholder="Search files and folders…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          autoComplete="off"
          spellCheck={false}
        />
        {query && (
          <button
            className="icon-btn"
            onClick={() => { setQuery(""); setResults([]); inputRef.current?.focus(); }}
            title="Clear search"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="12" cy="12" r="9" />
              <line x1="15" y1="9" x2="9" y2="15" />
              <line x1="9" y1="9" x2="15" y2="15" />
            </svg>
          </button>
        )}
        <button className="icon-btn" onClick={onSettings} title="Settings (Ctrl+,)">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
          </svg>
        </button>
        <button
          className="icon-btn close-btn"
          onClick={() => { setQuery(""); setResults([]); hideWindow(); }}
          title="Close (Esc)"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      </div>

      {/* Expanding panel - grows downward from the search bar */}
      <div className={`results-panel${query.trim() || isIndexing ? " open" : ""}`}>
        <div className="results-separator" />

        {/* Filter chips */}
        <div className="filter-bar">
          {FILTERS.map(f => (
            <button
              key={f.id}
              className={`filter-chip${filter === f.id ? " active" : ""}`}
              onClick={() => onFilterChange(f.id)}
            >
              {f.label}
            </button>
          ))}
        </div>

        <div className="results">
          {results.length === 0 && query.trim() ? (
            <div className="empty-state">No results for "{query}"</div>
          ) : results.length === 0 ? (
            <div className="empty-state">
              {isIndexing
                ? `Indexing… ${status?.files_indexed.toLocaleString()} files`
                : fileCount > 0
                ? `${fileCount.toLocaleString()} files indexed`
                : "No files indexed yet - open Settings to add folders"}
            </div>
          ) : (
            results.map((r, i) => (
              <ResultItem
                key={r.id}
                result={r}
                selected={i === selectedIdx}
                onSelect={() => setSelectedIdx(i)}
              />
            ))
          )}
        </div>

        {(results.length > 0 || isIndexing) && (
          <div className="status-bar">
            <div className={`status-dot${isIndexing ? " indexing" : ""}`} />
            <span className="status-text">
              {isIndexing
                ? `Indexing… ${status?.files_indexed.toLocaleString()} files`
                : `${results.length} result${results.length !== 1 ? "s" : ""}`}
            </span>
          </div>
        )}
      </div>
    </>
  );
}
