import { useEffect, useRef, useState } from "react";
import {
  Settings,
  IndexStatus,
  getSettings,
  saveSettings,
  startIndexing,
  getIndexStatus,
  getFileCount,
  pickFolder,
  getDrives,
  updateHotkey,
} from "../lib/api";

interface Props {
  onBack: () => void;
  theme: "dark" | "light";
  onThemeChange: (t: "dark" | "light") => void;
}

export default function SettingsView({ onBack, theme, onThemeChange }: Props) {
  const [settings, setSettings] = useState<Settings>({
    indexed_folders: [],
    excluded_folders: [],
    max_results: 50,
    theme: "dark",
    launch_at_startup: true,
    start_minimized: true,
    minimize_to_tray: true,
    hotkey: "ctrl+space",
    reindex_interval_hours: 0,
  });
  const [status, setStatus] = useState<IndexStatus | null>(null);
  const [fileCount, setFileCount] = useState(0);
  const [saved, setSaved] = useState(false);
  const [recordingHotkey, setRecordingHotkey] = useState(false);
  const [pendingHotkey, setPendingHotkey] = useState<string | null>(null);
  const pendingHotkeyRef = useRef<string | null>(null);

  useEffect(() => {
    getSettings().then(setSettings).catch(() => {});
    getIndexStatus().then(setStatus).catch(() => {});
    getFileCount().then(setFileCount).catch(() => {});
  }, []);

  // Poll while indexing
  useEffect(() => {
    if (!status?.is_indexing) return;
    const id = setInterval(async () => {
      const s = await getIndexStatus().catch(() => null);
      if (s) setStatus(s);
      const c = await getFileCount().catch(() => 0);
      setFileCount(c);
    }, 800);
    return () => clearInterval(id);
  }, [status?.is_indexing]);

  const handleSave = async () => {
    await saveSettings({ ...settings, theme });
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  const handleTheme = (t: "dark" | "light") => {
    onThemeChange(t);
    saveSettings({ ...settings, theme: t }).catch(() => {});
  };

  const handleIndex = async () => {
    await handleSave();
    await startIndexing().catch(() => {});
    const s = await getIndexStatus().catch(() => null);
    if (s) setStatus(s);
  };

  const addAllDrives = async () => {
    const drives = await getDrives().catch(() => [] as string[]);
    if (drives.length === 0) return;
    setSettings((s) => ({
      ...s,
      indexed_folders: [...new Set([...s.indexed_folders, ...drives])],
    }));
  };

  const addIndexedFolder = async () => {
    const folder = await pickFolder();
    if (folder && !settings.indexed_folders.includes(folder)) {
      setSettings((s) => ({ ...s, indexed_folders: [...s.indexed_folders, folder] }));
    }
  };

  const removeIndexedFolder = (path: string) => {
    setSettings((s) => ({
      ...s,
      indexed_folders: s.indexed_folders.filter((f) => f !== path),
    }));
  };

  const addExcludedFolder = async () => {
    const folder = await pickFolder();
    if (folder && !settings.excluded_folders.includes(folder)) {
      setSettings((s) => ({ ...s, excluded_folders: [...s.excluded_folders, folder] }));
    }
  };

  const removeExcludedFolder = (path: string) => {
    setSettings((s) => ({
      ...s,
      excluded_folders: s.excluded_folders.filter((f) => f !== path),
    }));
  };

  function formatHotkey(h: string): string {
    return h.split("+").map(k =>
      k === "ctrl" ? "Ctrl" : k === "alt" ? "Alt" :
      k === "shift" ? "Shift" : k === "meta" ? "Win" :
      k === "space" ? "Space" : k.toUpperCase()
    ).join(" + ");
  }

  const handleHotkeyKeyDown = (e: React.KeyboardEvent) => {
    e.preventDefault();
    const MODS = ["Control", "Alt", "Shift", "Meta"];
    if (MODS.includes(e.key)) return;
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("ctrl");
    if (e.altKey) parts.push("alt");
    if (e.shiftKey) parts.push("shift");
    if (e.metaKey) parts.push("meta");
    if (parts.length === 0) return;
    let key = e.code === "Space" ? "space"
      : e.code.startsWith("Key") ? e.code.slice(3).toLowerCase()
      : e.code.startsWith("Digit") ? e.code.slice(5)
      : e.code.startsWith("F") && e.code.length <= 3 ? e.code.toLowerCase()
      : e.code.toLowerCase();
    parts.push(key);
    const hk = parts.join("+");
    pendingHotkeyRef.current = hk;
    setPendingHotkey(hk);
  };

  const applyHotkey = async () => {
    const hk = pendingHotkeyRef.current;
    if (!hk) return;
    pendingHotkeyRef.current = null;
    setPendingHotkey(null);
    setRecordingHotkey(false);
    try {
      await updateHotkey(hk);
      const next = { ...settings, hotkey: hk };
      setSettings(next);
      saveSettings({ ...next, theme }).catch(() => {});
    } catch {}
  };

  const isIndexing = status?.is_indexing ?? false;

  return (
    <div className="settings-view">
      {/* Header */}
      <div className="settings-header">
        <button
          className="icon-btn"
          onClick={onBack}
          title="Back"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
        <span className="settings-title">Settings</span>
        <button
          className="icon-btn"
          onClick={handleSave}
          title="Save"
          style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
        >
          {saved ? (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#30d158" strokeWidth="2.5">
              <polyline points="20 6 9 17 4 12" />
            </svg>
          ) : (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z" />
              <polyline points="17 21 17 13 7 13 7 21" />
              <polyline points="7 3 7 8 15 8" />
            </svg>
          )}
        </button>
      </div>

      {/* Body */}
      <div className="settings-body">
        {/* Theme */}
        <div>
          <div className="settings-section-title">Appearance</div>
          <div className="theme-toggle">
            <button
              className={`theme-toggle-btn${theme === "dark" ? " active" : ""}`}
              onClick={() => handleTheme("dark")}
            >
              🌙 Dark
            </button>
            <button
              className={`theme-toggle-btn${theme === "light" ? " active" : ""}`}
              onClick={() => handleTheme("light")}
            >
              ☀️ Light
            </button>
          </div>
        </div>

        {/* Startup */}
        <div>
          <div className="settings-section-title">System</div>
          <label className="toggle-row">
            <span className="toggle-label">Launch at startup</span>
            <button
              className={`toggle-switch${settings.launch_at_startup ? " on" : ""}`}
              onClick={() =>
                setSettings((s) => {
                  const next = { ...s, launch_at_startup: !s.launch_at_startup };
                  saveSettings({ ...next, theme }).catch(() => {});
                  return next;
                })
              }
            />
          </label>
          <label className="toggle-row">
            <span className="toggle-label">Start minimized</span>
            <button
              className={`toggle-switch${settings.start_minimized ? " on" : ""}`}
              onClick={() =>
                setSettings((s) => {
                  const next = { ...s, start_minimized: !s.start_minimized };
                  saveSettings({ ...next, theme }).catch(() => {});
                  return next;
                })
              }
            />
          </label>
          <label className="toggle-row">
            <span className="toggle-label">Close minimizes to tray</span>
            <button
              className={`toggle-switch${settings.minimize_to_tray ? " on" : ""}`}
              onClick={() =>
                setSettings((s) => {
                  const next = { ...s, minimize_to_tray: !s.minimize_to_tray };
                  saveSettings({ ...next, theme }).catch(() => {});
                  return next;
                })
              }
            />
          </label>
        </div>

        {/* Global Shortcut */}
        <div>
          <div className="settings-section-title">Global Shortcut</div>
          <div className="hotkey-row">
            <div
              className={`hotkey-input${recordingHotkey ? " recording" : ""}`}
              tabIndex={0}
              onClick={() => { setRecordingHotkey(true); setPendingHotkey(null); }}
              onKeyDown={recordingHotkey ? handleHotkeyKeyDown : undefined}
              onBlur={() => { setRecordingHotkey(false); setPendingHotkey(null); pendingHotkeyRef.current = null; }}
            >
              {recordingHotkey
                ? (pendingHotkey ? formatHotkey(pendingHotkey) : "Press a combination…")
                : formatHotkey(settings.hotkey || "ctrl+space")}
            </div>
            {pendingHotkey && (
              <button
                className="primary-btn"
                style={{ padding: "6px 14px", fontSize: 12 }}
                onMouseDown={(e) => e.preventDefault()}
                onClick={applyHotkey}
              >
                Apply
              </button>
            )}
          </div>
        </div>

        {/* Auto Re-index */}
        <div>
          <div className="settings-section-title">Auto Re-index</div>
          <div className="theme-toggle">
            {[0, 1, 6, 12, 24].map(h => (
              <button
                key={h}
                className={`theme-toggle-btn${settings.reindex_interval_hours === h ? " active" : ""}`}
                onClick={() => setSettings(s => {
                  const next = { ...s, reindex_interval_hours: h };
                  saveSettings({ ...next, theme }).catch(() => {});
                  return next;
                })}
              >
                {h === 0 ? "Off" : `${h}h`}
              </button>
            ))}
          </div>
        </div>

        {/* Indexed folders */}
        <div>
          <div className="settings-section-title">Indexed Folders</div>
          <div className="folder-list">
            {settings.indexed_folders.map((f) => (
              <div className="folder-item" key={f}>
                <span style={{ fontSize: 14 }}>📁</span>
                <span className="folder-path">{f}</span>
                <button className="remove-btn" onClick={() => removeIndexedFolder(f)}>×</button>
              </div>
            ))}
          </div>
          <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
            <button className="add-btn" style={{ flex: 1 }} onClick={addIndexedFolder}>
              <span>＋</span> Add folder
            </button>
            <button className="add-btn" style={{ flex: 1 }} onClick={addAllDrives} title="Add every drive root (C:\, D:\, …)">
              <span>💾</span> Add all drives
            </button>
          </div>
        </div>

        {/* Excluded folders */}
        <div>
          <div className="settings-section-title">Excluded Folders</div>
          <div className="folder-list">
            {settings.excluded_folders.map((f) => (
              <div className="folder-item" key={f}>
                <span style={{ fontSize: 14 }}>🚫</span>
                <span className="folder-path">{f}</span>
                <button className="remove-btn" onClick={() => removeExcludedFolder(f)}>×</button>
              </div>
            ))}
          </div>
          <button className="add-btn" onClick={addExcludedFolder}>
            <span>＋</span> Add exclusion
          </button>
        </div>
      </div>

      {/* Footer */}
      <div className="settings-footer">
        <button
          className="primary-btn"
          onClick={handleIndex}
          disabled={isIndexing || settings.indexed_folders.length === 0}
        >
          {isIndexing ? "Indexing…" : "Start Indexing"}
        </button>
        <span className="indexing-status">
          {isIndexing
            ? `${status?.files_indexed.toLocaleString()} files… ${status?.current_path.split("\\").pop() ?? ""}`
            : fileCount > 0
            ? `${fileCount.toLocaleString()} files in index`
            : settings.indexed_folders.length === 0
            ? "Add folders to index above"
            : "Click Start Indexing to build the index"}
        </span>
      </div>
    </div>
  );
}
