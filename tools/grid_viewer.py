"""PyQt Grid Media Viewer for Director game files.

Displays all cast members (bitmaps, sounds, texts, shapes, palettes,
scripts, buttons, transitions, etc.) in a filterable, searchable grid.
"""

from __future__ import annotations

import logging
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from PyQt5.QtCore import Qt, QTimer
from PyQt5.QtGui import (
    QColor,
    QFont,
    QImage,
    QKeySequence,
    QPainter,
    QPixmap,
)
from PyQt5.QtWidgets import (
    QApplication,
    QComboBox,
    QFileDialog,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QProgressDialog,
    QPushButton,
    QScrollArea,
    QStatusBar,
    QVBoxLayout,
    QWidget,
    QFrame,
    QShortcut,
)

# ---------------------------------------------------------------------------
# willy_re imports – adjust sys.path so it works from tools/ folder
# ---------------------------------------------------------------------------
_TOOLS_DIR = Path(__file__).resolve().parent
_WILLY_RE_DIR = _TOOLS_DIR / "willy_re"
if str(_WILLY_RE_DIR) not in sys.path:
    sys.path.insert(0, str(_WILLY_RE_DIR))

from willy_re.director.parser import DirectorFile, CastMember
from willy_re.director.bitmap import bitd_to_image
from willy_re.director.chunks import CAST_TYPE_NAMES, CastType
from willy_re.director.external_casts import load_external_casts
from willy_re.director.text import parse_stxt

logging.basicConfig(level=logging.WARNING)
log = logging.getLogger(__name__)

# Default game directory
DEFAULT_GAME_DIR = Path(__file__).resolve().parent.parent / "game"

THUMB_SIZE = 128  # thumbnail side in pixels
GRID_COLS = 6  # default column count

# Colour badges per type
TYPE_COLOURS: dict[int, str] = {
    CastType.BITMAP: "#4CAF50",
    CastType.SOUND: "#2196F3",
    CastType.TEXT: "#FF9800",
    CastType.FIELD: "#FF9800",
    CastType.SHAPE: "#9C27B0",
    CastType.BUTTON: "#E91E63",
    CastType.PALETTE: "#795548",
    CastType.SCRIPT: "#607D8B",
    CastType.TRANSITION: "#00BCD4",
    CastType.DIGITAL_VIDEO: "#F44336",
    CastType.FILMLOOP: "#8BC34A",
    CastType.PICTURE: "#CDDC39",
    CastType.MOVIE: "#FF5722",
    CastType.OLE: "#9E9E9E",
    CastType.NULL: "#BDBDBD",
}


# ---------------------------------------------------------------------------
# MediaItem — one item in the grid
# ---------------------------------------------------------------------------


@dataclass
class MediaItem:
    """A resolved cast member ready for display."""

    member: CastMember
    lib_name: str
    slot: int
    source_file: str  # base name of the Director file
    thumbnail: QPixmap | None = None
    description: str = ""
    pil_image: Any = None  # PIL Image for clipboard copy


# ---------------------------------------------------------------------------
# Loading helpers
# ---------------------------------------------------------------------------


def _resolve_palette(
    dir_file: DirectorFile,
    lib_name: str,
    palette_id: int,
) -> list[tuple[int, int, int]] | None:
    if palette_id <= 0:
        return None
    pal_member = dir_file.get_member(lib_name, palette_id)
    if pal_member and pal_member.palette_data:
        return pal_member.palette_data
    for lib in dir_file.cast_libraries:
        pm = lib.members.get(palette_id)
        if pm and pm.palette_data:
            return pm.palette_data
    for _ext_name, ext_df in dir_file.external_casts.items():
        for elib in ext_df.cast_libraries:
            pm = elib.members.get(palette_id)
            if pm and pm.palette_data:
                return pm.palette_data
    return None


def _pil_to_qpixmap(pil_img, max_size: int = THUMB_SIZE) -> QPixmap:
    """Convert a PIL Image to a QPixmap thumbnail."""
    # Convert paletted/1-bit images to RGB first to avoid palette issues
    if pil_img.mode == "P":
        img = pil_img.convert("RGB")
    elif pil_img.mode == "1":
        img = pil_img.convert("RGB")
    elif pil_img.mode == "RGBA":
        img = pil_img
    else:
        img = pil_img.convert("RGBA")

    if img.mode == "RGBA":
        data = img.tobytes("raw", "RGBA")
        qimg = QImage(
            data, img.width, img.height, 4 * img.width, QImage.Format_RGBA8888
        )
    else:
        data = img.tobytes("raw", "RGB")
        qimg = QImage(data, img.width, img.height, 3 * img.width, QImage.Format_RGB888)

    # .copy() ensures the QImage owns its data (avoids dangling buffer)
    qimg = qimg.copy()
    pix = QPixmap.fromImage(qimg)
    if pix.width() > max_size or pix.height() > max_size:
        pix = pix.scaled(
            max_size, max_size, Qt.KeepAspectRatio, Qt.SmoothTransformation
        )
    return pix


def _make_placeholder(text: str, colour: str, size: int = THUMB_SIZE) -> QPixmap:
    """Create a placeholder pixmap with centred text."""
    pix = QPixmap(size, size)
    pix.fill(QColor(colour))
    painter = QPainter(pix)
    painter.setPen(QColor("white"))
    font = QFont("Segoe UI", 10, QFont.Bold)
    painter.setFont(font)
    painter.drawText(pix.rect(), Qt.AlignCenter | Qt.TextWordWrap, text)
    painter.end()
    return pix


def _load_bitmap_thumb(
    dir_file: DirectorFile,
    member: CastMember,
    lib_name: str,
) -> tuple[QPixmap | None, Any]:
    """Try to decode a bitmap cast member. Returns (thumbnail, pil_image)."""
    if member.image_width <= 0 or member.image_height <= 0:
        return None, None

    for slot in member.linked_entries:
        if slot >= len(dir_file.entries):
            continue
        entry = dir_file.entries[slot]
        if entry.type != "BITD":
            continue
        try:
            with open(dir_file.path, "rb") as f:
                palette = _resolve_palette(dir_file, lib_name, member.image_palette)
                img = bitd_to_image(
                    f,
                    entry.data_offset,
                    entry.data_length,
                    member.image_width,
                    member.image_height,
                    member.image_bit_depth,
                    palette=palette,
                    palette_id=member.image_palette,
                    transparent_white=False,
                    is_windows=dir_file.little_endian,
                )
                if img:
                    return _pil_to_qpixmap(img), img
        except Exception as e:
            log.warning("Bitmap decode failed %s/%d: %s", lib_name, member.slot, e)
    return None, None


def _load_text_preview(
    dir_file: DirectorFile,
    member: CastMember,
) -> str:
    """Extract STXT text content for preview."""
    for slot in member.linked_entries:
        if slot >= len(dir_file.entries):
            continue
        entry = dir_file.entries[slot]
        if entry.type != "STXT":
            continue
        try:
            raw = dir_file.get_entry_data(slot)
            result = parse_stxt(raw)
            return result.text[:200]
        except Exception:
            pass
    return ""


def load_director_file(path: Path) -> DirectorFile:
    """Parse a Director file with external casts."""
    df = DirectorFile(path)
    df.parse()
    try:
        df.external_casts = load_external_casts(df)
    except Exception:
        df.external_casts = {}
    return df


def collect_items(
    dir_file: DirectorFile,
    source_name: str,
    *,
    seen_external: set[str] | None = None,
) -> list[MediaItem]:
    """Collect all cast members from a parsed DirectorFile into MediaItems.

    Args:
        seen_external: When loading "All Files", pass a shared set so that
            external casts referenced by multiple DXR files are only
            included once.
    """
    items: list[MediaItem] = []

    def _process_libs(df: DirectorFile, src: str):
        for lib in df.cast_libraries:
            for num, member in lib.members.items():
                if member.cast_type == CastType.NULL:
                    continue

                item = MediaItem(
                    member=member,
                    lib_name=lib.name,
                    slot=num,
                    source_file=src,
                )

                ct = member.cast_type
                type_name = CAST_TYPE_NAMES.get(ct, f"Unknown({ct})")
                colour = TYPE_COLOURS.get(ct, "#757575")

                if ct == CastType.BITMAP:
                    thumb, pil_img = _load_bitmap_thumb(df, member, lib.name)
                    if thumb:
                        item.thumbnail = thumb
                        item.pil_image = pil_img
                    else:
                        item.thumbnail = _make_placeholder(
                            f"Bitmap\n{member.image_width}x{member.image_height}",
                            colour,
                        )
                    item.description = (
                        f"{member.image_width}x{member.image_height} "
                        f"{member.image_bit_depth}bpp"
                    )

                elif ct == CastType.SOUND:
                    dur = member.sound_duration_seconds
                    item.thumbnail = _make_placeholder(
                        f"Sound\n{dur:.1f}s\n{member.sound_sample_rate}Hz",
                        colour,
                    )
                    item.description = (
                        f"{member.sound_sample_rate}Hz "
                        f"{member.sound_sample_size}bit "
                        f"{member.sound_channels}ch "
                        f"{dur:.1f}s"
                    )

                elif ct in (CastType.TEXT, CastType.FIELD):
                    text = _load_text_preview(df, member)
                    preview = text[:60].replace("\n", " ") if text else "(empty)"
                    item.thumbnail = _make_placeholder(
                        f"Text\n{preview[:40]}",
                        colour,
                    )
                    item.description = preview

                elif ct == CastType.SHAPE:
                    shape_label = "Shape"
                    if member.shape_data:
                        shape_label = str(member.shape_data)
                    item.thumbnail = _make_placeholder(shape_label, colour)
                    item.description = shape_label

                elif ct == CastType.BUTTON:
                    item.thumbnail = _make_placeholder("Button", colour)
                    item.description = "Button"

                elif ct == CastType.PALETTE:
                    # Render palette swatch
                    if member.palette_data:
                        item.thumbnail = _render_palette_thumb(member.palette_data)
                        item.description = (
                            f"Palette ({len(member.palette_data)} colours)"
                        )
                    else:
                        item.thumbnail = _make_placeholder("Palette", colour)
                        item.description = "Palette"

                elif ct == CastType.SCRIPT:
                    item.thumbnail = _make_placeholder("Script", colour)
                    item.description = "Lingo Script"

                elif ct == CastType.TRANSITION:
                    item.thumbnail = _make_placeholder("Transition", colour)
                    item.description = "Transition"

                elif ct == CastType.DIGITAL_VIDEO:
                    item.thumbnail = _make_placeholder("Video", colour)
                    item.description = "Digital Video"

                elif ct == CastType.FILMLOOP:
                    item.thumbnail = _make_placeholder("Film Loop", colour)
                    item.description = "Film Loop"

                elif ct == CastType.PICTURE:
                    item.thumbnail = _make_placeholder("Picture", colour)
                    item.description = "Picture"

                else:
                    item.thumbnail = _make_placeholder(type_name, colour)
                    item.description = type_name

                items.append(item)

    _process_libs(dir_file, source_name)

    for ext_name, ext_df in dir_file.external_casts.items():
        if seen_external is not None:
            ext_key = str(Path(ext_df.path).resolve())
            if ext_key in seen_external:
                continue
            seen_external.add(ext_key)
        _process_libs(ext_df, f"{source_name} → {ext_name}")

    return items


def _render_palette_thumb(
    palette: list[tuple[int, int, int]], size: int = THUMB_SIZE
) -> QPixmap:
    """Render a palette as a colour swatch grid."""
    pix = QPixmap(size, size)
    pix.fill(QColor("black"))
    painter = QPainter(pix)
    cols = 16
    rows = (len(palette) + cols - 1) // cols
    cell_w = size / cols
    cell_h = size / max(rows, 1)
    for i, (r, g, b) in enumerate(palette):
        x = (i % cols) * cell_w
        y = (i // cols) * cell_h
        painter.fillRect(
            int(x), int(y), max(1, int(cell_w)), max(1, int(cell_h)), QColor(r, g, b)
        )
    painter.end()
    return pix


# ---------------------------------------------------------------------------
# Card Widget — one cell in the grid
# ---------------------------------------------------------------------------


class MediaCard(QFrame):
    """A clickable card showing one cast member."""

    def __init__(self, item: MediaItem, parent=None):
        super().__init__(parent)
        self.item = item
        self._selected = False

        self.setFrameStyle(QFrame.Box | QFrame.Plain)
        self.setLineWidth(1)
        self.setFixedSize(THUMB_SIZE + 24, THUMB_SIZE + 68)
        self.setCursor(Qt.PointingHandCursor)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(4, 4, 4, 4)
        layout.setSpacing(2)

        # Thumbnail
        self.thumb_label = QLabel()
        self.thumb_label.setAlignment(Qt.AlignCenter)
        self.thumb_label.setFixedSize(THUMB_SIZE + 12, THUMB_SIZE + 4)
        if item.thumbnail:
            self.thumb_label.setPixmap(item.thumbnail)
        layout.addWidget(self.thumb_label)

        # ID + Name label
        name = item.member.name or "(unnamed)"
        type_name = CAST_TYPE_NAMES.get(item.member.cast_type, "?")
        colour = TYPE_COLOURS.get(item.member.cast_type, "#757575")
        id_text = f"<b style='color:{colour}'>[{type_name}]</b> #{item.slot}"
        name_text = f"<small>{_elide(name, 18)}</small>"

        info_label = QLabel(f"{id_text}<br>{name_text}")
        info_label.setAlignment(Qt.AlignCenter)
        info_label.setWordWrap(True)
        info_label.setTextFormat(Qt.RichText)
        info_label.setStyleSheet("font-size: 10px;")
        layout.addWidget(info_label)

        self._update_style()

    def _update_style(self):
        if self._selected:
            self.setStyleSheet(
                "MediaCard { background: #e3f2fd; border: 2px solid #1976D2; }"
            )
        else:
            self.setStyleSheet(
                "MediaCard { background: #fafafa; border: 1px solid #e0e0e0; }"
                "MediaCard:hover { background: #f0f0f0; border: 1px solid #bdbdbd; }"
            )

    def set_selected(self, sel: bool):
        self._selected = sel
        self._update_style()

    def mousePressEvent(self, ev):
        # Signal selection to parent
        parent = self.parent()
        while parent and not isinstance(parent, GridPanel):
            parent = parent.parent()
        if parent:
            parent.select_card(self)
        super().mousePressEvent(ev)

    def mouseDoubleClickEvent(self, ev):
        parent = self.parent()
        while parent and not isinstance(parent, GridPanel):
            parent = parent.parent()
        if parent:
            parent.copy_selected()
        super().mouseDoubleClickEvent(ev)


def _elide(text: str, max_len: int) -> str:
    if len(text) <= max_len:
        return text
    return text[: max_len - 1] + "…"


# ---------------------------------------------------------------------------
# Grid Panel — scrollable grid of cards
# ---------------------------------------------------------------------------


class GridPanel(QScrollArea):
    """Scrollable area containing a grid of MediaCards."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWidgetResizable(True)
        self._cards: list[MediaCard] = []
        self._selected: MediaCard | None = None
        self._container = QWidget()
        self._grid = QGridLayout(self._container)
        self._grid.setSpacing(6)
        self._grid.setContentsMargins(8, 8, 8, 8)
        self.setWidget(self._container)

        # Debounce timer for resize relayout
        self._resize_timer = QTimer(self)
        self._resize_timer.setSingleShot(True)
        self._resize_timer.setInterval(150)
        self._resize_timer.timeout.connect(self._relayout)

    def set_items(self, items: list[MediaItem]):
        """Populate the grid with items."""
        # Clear old
        for card in self._cards:
            card.setParent(None)
            card.deleteLater()
        self._cards.clear()
        self._selected = None

        # Calculate columns based on viewport width
        cols = max(1, (self.viewport().width() - 16) // (THUMB_SIZE + 30))

        for i, item in enumerate(items):
            card = MediaCard(item, self._container)
            self._grid.addWidget(card, i // cols, i % cols)
            self._cards.append(card)

    def select_card(self, card: MediaCard):
        if self._selected:
            self._selected.set_selected(False)
        self._selected = card
        card.set_selected(True)
        # Update status bar
        main = self.window()
        if isinstance(main, MainWindow):
            item = card.item
            main.statusBar().showMessage(
                f"Selected: #{item.slot} '{item.member.name}' "
                f"[{item.member.type_name}] — {item.description}  "
                f"(Source: {item.source_file}, Lib: {item.lib_name})"
            )

    def copy_selected(self):
        if not self._selected:
            return
        item = self._selected.item
        clipboard = QApplication.clipboard()

        if item.pil_image:
            # Copy as image
            if item.pil_image.mode == "P":
                rgb = item.pil_image.convert("RGB")
                data = rgb.tobytes("raw", "RGB")
                qimg = QImage(
                    data, rgb.width, rgb.height, 3 * rgb.width, QImage.Format_RGB888
                )
            else:
                rgba = item.pil_image.convert("RGBA")
                data = rgba.tobytes("raw", "RGBA")
                qimg = QImage(
                    data,
                    rgba.width,
                    rgba.height,
                    4 * rgba.width,
                    QImage.Format_RGBA8888,
                )
            clipboard.setImage(qimg.copy())
            self.window().statusBar().showMessage(
                f"Copied bitmap #{item.slot} '{item.member.name}' to clipboard",
                3000,
            )
        else:
            # Copy metadata as text
            info = (
                f"ID: {item.slot}\n"
                f"Name: {item.member.name}\n"
                f"Type: {item.member.type_name}\n"
                f"Library: {item.lib_name}\n"
                f"Source: {item.source_file}\n"
                f"Description: {item.description}"
            )
            clipboard.setText(info)
            self.window().statusBar().showMessage(
                f"Copied info for #{item.slot} '{item.member.name}' to clipboard",
                3000,
            )

    def resizeEvent(self, ev):
        super().resizeEvent(ev)
        # Debounce relayout to avoid expensive per-pixel re-gridding
        self._resize_timer.start()

    def _relayout(self):
        """Re-grid all cards to match current viewport width."""
        if not self._cards:
            return
        cols = max(1, (self.viewport().width() - 16) // (THUMB_SIZE + 30))
        for i, card in enumerate(self._cards):
            self._grid.removeWidget(card)
            self._grid.addWidget(card, i // cols, i % cols)


# ---------------------------------------------------------------------------
# Main Window
# ---------------------------------------------------------------------------


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("OpenWilly Grid Media Viewer")
        self.resize(1280, 800)

        self._all_items: list[MediaItem] = []
        self._filtered_items: list[MediaItem] = []

        # Central widget
        central = QWidget()
        self.setCentralWidget(central)
        main_layout = QVBoxLayout(central)
        main_layout.setContentsMargins(0, 0, 0, 0)

        # ---- Toolbar ----
        toolbar = QWidget()
        tb_layout = QHBoxLayout(toolbar)
        tb_layout.setContentsMargins(8, 4, 8, 4)

        # Open folder button
        open_btn = QPushButton("Open Game Dir…")
        open_btn.clicked.connect(self._open_dir)
        tb_layout.addWidget(open_btn)

        # File selector
        tb_layout.addWidget(QLabel("File:"))
        self._file_combo = QComboBox()
        self._file_combo.setMinimumWidth(200)
        self._file_combo.currentIndexChanged.connect(self._on_file_changed)
        tb_layout.addWidget(self._file_combo)

        # Type filter
        tb_layout.addWidget(QLabel("Type:"))
        self._type_combo = QComboBox()
        self._type_combo.addItem("All Types", -1)
        for ct_val, ct_name in sorted(CAST_TYPE_NAMES.items()):
            if ct_val == 0:
                continue
            self._type_combo.addItem(ct_name, ct_val)
        self._type_combo.currentIndexChanged.connect(self._apply_filter)
        tb_layout.addWidget(self._type_combo)

        # Search
        tb_layout.addWidget(QLabel("Search:"))
        self._search = QLineEdit()
        self._search.setPlaceholderText("Filter by name or ID…")
        self._search.textChanged.connect(self._apply_filter)
        self._search.setClearButtonEnabled(True)
        tb_layout.addWidget(self._search, 1)

        # Copy button
        copy_btn = QPushButton("Copy Selected (Ctrl+C)")
        copy_btn.clicked.connect(self._copy)
        tb_layout.addWidget(copy_btn)

        # Count label
        self._count_label = QLabel("0 items")
        tb_layout.addWidget(self._count_label)

        main_layout.addWidget(toolbar)

        # ---- Grid ----
        self._grid = GridPanel()
        main_layout.addWidget(self._grid)

        # ---- Status bar ----
        self.setStatusBar(QStatusBar())

        # Keyboard shortcuts
        QShortcut(QKeySequence("Ctrl+C"), self, self._copy)
        QShortcut(QKeySequence("Ctrl+O"), self, self._open_dir)

        # State
        self._game_dir: Path | None = None
        self._dir_files: list[Path] = []
        self._loaded_items: dict[str, list[MediaItem]] = {}  # file name → items

        # Try default game dir
        if DEFAULT_GAME_DIR.exists():
            QTimer.singleShot(100, lambda: self._load_game_dir(DEFAULT_GAME_DIR))

    # -- Actions ---------------------------------------------------------------

    def _open_dir(self):
        d = QFileDialog.getExistingDirectory(
            self, "Select Game Directory", str(DEFAULT_GAME_DIR)
        )
        if d:
            self._load_game_dir(Path(d))

    def _load_game_dir(self, game_dir: Path):
        self._game_dir = game_dir
        self._dir_files = sorted(
            game_dir.rglob("*"),
            key=lambda p: p.name.lower(),
        )
        self._dir_files = [
            p
            for p in self._dir_files
            if p.suffix.upper() in (".DXR", ".CXT", ".CST", ".DIR")
        ]

        self._file_combo.blockSignals(True)
        self._file_combo.clear()
        self._file_combo.addItem("(All Files)", "ALL")
        for p in self._dir_files:
            rel = p.relative_to(game_dir)
            self._file_combo.addItem(str(rel), str(p))
        self._file_combo.blockSignals(False)

        self._loaded_items.clear()
        self.statusBar().showMessage(
            f"Found {len(self._dir_files)} Director files in {game_dir}"
        )

        # Load first file by default
        if self._dir_files:
            self._file_combo.setCurrentIndex(1)  # first actual file

    def _on_file_changed(self, index: int):
        if index < 0:
            return

        key = self._file_combo.itemData(index)
        if key == "ALL":
            self._load_all_files()
        else:
            self._load_single_file(Path(key))

    def _load_single_file(self, path: Path):
        key = str(path)
        if key not in self._loaded_items:
            self.statusBar().showMessage(f"Loading {path.name}…")
            QApplication.processEvents()
            try:
                df = load_director_file(path)
                items = collect_items(df, path.name)
                self._loaded_items[key] = items
                df.close()
            except Exception as e:
                QMessageBox.warning(
                    self, "Load Error", f"Failed to load {path.name}:\n{e}"
                )
                self._loaded_items[key] = []

        self._all_items = self._loaded_items[key]
        self._apply_filter()
        self.statusBar().showMessage(
            f"Loaded {path.name}: {len(self._all_items)} cast members"
        )

    def _load_all_files(self):
        """Load all Director files and merge items."""
        progress = QProgressDialog(
            "Loading Director files…", "Cancel", 0, len(self._dir_files), self
        )
        progress.setWindowModality(Qt.WindowModal)
        progress.setMinimumDuration(0)

        all_items: list[MediaItem] = []
        seen_external: set[str] = set()
        for i, path in enumerate(self._dir_files):
            if progress.wasCanceled():
                break
            progress.setValue(i)
            progress.setLabelText(f"Loading {path.name}…")
            QApplication.processEvents()

            key = str(path)
            if key not in self._loaded_items:
                try:
                    df = load_director_file(path)
                    items = collect_items(
                        df,
                        path.name,
                        seen_external=seen_external,
                    )
                    self._loaded_items[key] = items
                    df.close()
                except Exception as e:
                    log.warning("Failed to load %s: %s", path.name, e)
                    self._loaded_items[key] = []

            all_items.extend(self._loaded_items[key])

        progress.setValue(len(self._dir_files))
        self._all_items = all_items
        self._apply_filter()
        self.statusBar().showMessage(
            f"Loaded all files: {len(self._all_items)} cast members total"
        )

    def _apply_filter(self):
        type_filter = self._type_combo.currentData()
        search = self._search.text().strip().lower()

        items = self._all_items
        if type_filter is not None and type_filter != -1:
            items = [i for i in items if i.member.cast_type == type_filter]
        if search:
            items = [
                i
                for i in items
                if search in (i.member.name or "").lower()
                or search in str(i.slot)
                or search in i.description.lower()
                or search in i.source_file.lower()
            ]

        self._filtered_items = items
        self._grid.set_items(items)
        self._count_label.setText(f"{len(items)} items")

    def _copy(self):
        self._grid.copy_selected()


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main():
    app = QApplication(sys.argv)
    app.setStyle("Fusion")

    # Application-wide stylesheet
    app.setStyleSheet("""
        QMainWindow { background: #f5f5f5; }
        QToolBar { background: #ffffff; border-bottom: 1px solid #e0e0e0; }
        QStatusBar { background: #ffffff; border-top: 1px solid #e0e0e0; }
        QComboBox { min-height: 24px; }
        QLineEdit { min-height: 24px; }
        QPushButton { min-height: 26px; padding: 2px 12px; }
    """)

    win = MainWindow()
    win.show()
    sys.exit(app.exec_())


if __name__ == "__main__":
    main()
