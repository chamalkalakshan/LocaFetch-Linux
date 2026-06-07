import { useEffect, useRef, useState } from "react";
import { FileResult, formatSize, formatDate, openFile, revealInExplorer, copyPath, openWithDialog } from "../lib/api";

interface Props {
  result: FileResult;
  selected: boolean;
  onSelect: () => void;
}

function fileIcon(result: FileResult): string {
  if (result.is_dir) return "📁";
  const ext = result.extension?.toLowerCase();
  if (!ext) return "📄";
  if (["jpg","jpeg","png","gif","webp","svg","bmp","ico"].includes(ext)) return "🖼️";
  if (["mp4","mov","avi","mkv","webm","wmv"].includes(ext)) return "🎬";
  if (["mp3","wav","flac","aac","ogg","m4a"].includes(ext)) return "🎵";
  if (["pdf"].includes(ext)) return "📕";
  if (["zip","rar","7z","tar","gz"].includes(ext)) return "📦";
  if (["doc","docx"].includes(ext)) return "📝";
  if (["xls","xlsx"].includes(ext)) return "📊";
  if (["ppt","pptx"].includes(ext)) return "📊";
  if (["js","ts","jsx","tsx","py","rs","go","java","c","cpp","cs","rb","php"].includes(ext)) return "💻";
  if (["html","css","json","xml","yaml","yml","toml"].includes(ext)) return "📋";
  if (["txt","md","log"].includes(ext)) return "📄";
  if (["exe","msi","bat","cmd","ps1"].includes(ext)) return "⚙️";
  return "📄";
}

interface MenuState {
  x: number;
  y: number;
}

export default function ResultItem({ result, selected, onSelect }: Props) {
  const [menu, setMenu] = useState<MenuState | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const handleOpen = (e: React.MouseEvent) => {
    e.stopPropagation();
    openFile(result.path);
  };

  const handleReveal = (e: React.MouseEvent) => {
    e.stopPropagation();
    revealInExplorer(result.path);
  };

  const handleCopy = (e: React.MouseEvent) => {
    e.stopPropagation();
    copyPath(result.path);
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    onSelect();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const closeMenu = () => setMenu(null);

  const menuAction = (fn: () => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn();
    closeMenu();
  };

  useEffect(() => {
    if (!menu) return;
    const handler = () => closeMenu();
    document.addEventListener("mousedown", handler);
    document.addEventListener("keydown", handler);
    return () => {
      document.removeEventListener("mousedown", handler);
      document.removeEventListener("keydown", handler);
    };
  }, [menu]);

  // Keep menu inside viewport vertically
  const menuStyle = menu
    ? {
        left: menu.x,
        top: Math.min(menu.y, window.innerHeight - 160),
      }
    : undefined;

  const dir = result.path.substring(0, result.path.length - result.filename.length - 1);

  return (
    <>
      <div
        className={`result-item${selected ? " selected" : ""}`}
        onClick={handleOpen}
        onMouseEnter={onSelect}
        onContextMenu={handleContextMenu}
      >
        <div className="result-icon">{fileIcon(result)}</div>

        <div className="result-text">
          <div className="result-name">{result.filename}</div>
          <div className="result-path">{dir}</div>
        </div>

        <div className="result-meta">
          {!result.is_dir && formatSize(result.size)}
          {result.modified_time && (
            <div>{formatDate(result.modified_time)}</div>
          )}
        </div>

        <div className="result-actions">
          <button className="action-btn" onClick={handleOpen} title="Open">
            Open
          </button>
          <button className="action-btn" onClick={handleReveal} title="Reveal in Explorer">
            Reveal
          </button>
          <button className="action-btn" onClick={handleCopy} title="Copy path">
            Copy path
          </button>
        </div>
      </div>

      {menu && (
        <div
          ref={menuRef}
          className="context-menu"
          style={menuStyle}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <button className="context-item" onClick={menuAction(() => openFile(result.path))}>
            Open
          </button>
          {!result.is_dir && (
            <button className="context-item" onClick={menuAction(() => openWithDialog(result.path))}>
              Open with…
            </button>
          )}
          <div className="context-separator" />
          <button className="context-item" onClick={menuAction(() => revealInExplorer(result.path))}>
            Reveal in Explorer
          </button>
          <button className="context-item" onClick={menuAction(() => copyPath(result.path))}>
            Copy path
          </button>
        </div>
      )}
    </>
  );
}
