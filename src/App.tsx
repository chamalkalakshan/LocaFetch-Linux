import { useState, useEffect } from "react";
import SearchView from "./components/SearchView";
import SettingsView from "./components/SettingsView";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { getSettings } from "./lib/api";

type View = "search" | "settings";
type Theme = "dark" | "light";

const SETTINGS_H = 520;
const COLLAPSED_H = 68;

export default function App() {
  const [view, setView] = useState<View>("search");
  const [theme, setTheme] = useState<Theme>("dark");
  const [filter, setFilter] = useState("all");

  useEffect(() => {
    getSettings()
      .then((s) => setTheme(s.theme ?? "dark"))
      .catch(() => {});
  }, []);

  // Reset to search view and clear filter when window is shown from hidden state
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    win.listen("search-shown", () => {
      setView("search");
      setFilter("all");
      win.setSize(new LogicalSize(680, COLLAPSED_H)).catch(() => {});
    }).then((fn) => { unlisten = fn; });
    return () => { unlisten?.(); };
  }, []);

  const goSettings = () => {
    getCurrentWindow().setSize(new LogicalSize(680, SETTINGS_H)).catch(() => {});
    setView("settings");
  };

  const goSearch = () => {
    getCurrentWindow().setSize(new LogicalSize(680, COLLAPSED_H)).catch(() => {});
    setView("search");
  };

  return (
    <div className={`app${theme === "light" ? " light" : ""}`}>
      {view === "search" ? (
        <SearchView onSettings={goSettings} filter={filter} onFilterChange={setFilter} />
      ) : (
        <SettingsView onBack={goSearch} theme={theme} onThemeChange={setTheme} />
      )}
    </div>
  );
}
