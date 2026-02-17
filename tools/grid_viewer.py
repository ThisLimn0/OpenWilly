"""PyQt Grid Media Viewer for Director game files.

Displays all cast members (bitmaps, sounds, texts, shapes, palettes,
scripts, buttons, transitions, etc.) in a filterable, searchable grid.
"""

from __future__ import annotations

import logging
import os
import sys
import tempfile
import winsound
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
    QDialog,
    QFileDialog,
    QGridLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMenu,
    QMessageBox,
    QProgressDialog,
    QPushButton,
    QScrollArea,
    QSizePolicy,
    QSplitter,
    QStatusBar,
    QTextEdit,
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

from willy_re.director.parser import DirectorFile, CastMember  # type: ignore[import-not-found]  # noqa: E402
from willy_re.director.bitmap import bitd_to_image  # type: ignore[import-not-found]  # noqa: E402
from willy_re.director.chunks import CAST_TYPE_NAMES, CastType  # type: ignore[import-not-found]  # noqa: E402
from willy_re.director.external_casts import load_external_casts  # type: ignore[import-not-found]  # noqa: E402
from willy_re.director.sound import extract_snds_wav, extract_snd_wav  # type: ignore[import-not-found]  # noqa: E402
from willy_re.director.text import parse_stxt  # type: ignore[import-not-found]  # noqa: E402

logging.basicConfig(level=logging.WARNING)
log = logging.getLogger(__name__)

# Default game directory
DEFAULT_GAME_DIR = Path(__file__).resolve().parent.parent / "game"

THUMB_SIZE = 200  # thumbnail side in pixels
GRID_SPACING = 2  # pixels between grid cards
CARD_PAD = 2  # internal card padding

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
    wav_data: bytes | None = None  # Extracted WAV for sound playback


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
            max_size,
            max_size,
            Qt.AspectRatioMode.KeepAspectRatio,
            Qt.TransformationMode.SmoothTransformation,
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
    painter.drawText(
        pix.rect(), Qt.AlignmentFlag.AlignCenter | Qt.TextFlag.TextWordWrap, text
    )
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
                # If a custom palette ref could not be resolved, fall
                # back to the system palette instead of greyscale.
                pal_id = member.image_palette if palette else 0
                img = bitd_to_image(
                    f,
                    entry.data_offset,
                    entry.data_length,
                    member.image_width,
                    member.image_height,
                    member.image_bit_depth,
                    palette=palette,
                    palette_id=pal_id,
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


def _extract_sound_data(
    dir_file: DirectorFile,
    member: CastMember,
) -> bytes | None:
    """Extract a sound member's audio as WAV bytes."""
    for slot in member.linked_entries:
        if slot >= len(dir_file.entries):
            continue
        entry = dir_file.entries[slot]
        if entry.data_length == 0:
            continue
        try:
            with open(dir_file.path, "rb") as f:
                if entry.type == "sndS":
                    return extract_snds_wav(
                        f,
                        entry.data_offset,
                        entry.data_length,
                        member.sound_sample_rate,
                        member.sound_channels,
                        member.sound_sample_size // 8,
                    )
                elif entry.type == "snd ":
                    return extract_snd_wav(
                        f,
                        entry.data_offset,
                        entry.data_length,
                        member.sound_sample_rate,
                        member.sound_sample_size,
                        member.sound_data_length,
                        member.sound_channels,
                    )
        except Exception as e:
            log.warning("Sound extract failed %d: %s", member.slot, e)
    return None


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
                        f"\u25b6 Sound\n{dur:.1f}s\n{member.sound_sample_rate}Hz",
                        colour,
                    )
                    item.description = (
                        f"{member.sound_sample_rate}Hz "
                        f"{member.sound_sample_size}bit "
                        f"{member.sound_channels}ch "
                        f"{dur:.1f}s"
                    )
                    item.wav_data = _extract_sound_data(df, member)

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
# Bitmap Detail Dialog
# ---------------------------------------------------------------------------


class _CheckerboardLabel(QLabel):
    """QLabel that draws a checkerboard behind the pixmap for transparency."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setAlignment(Qt.AlignmentFlag.AlignCenter)

    def paintEvent(self, a0):  # type: ignore[override]
        painter = QPainter(self)
        # Draw checkerboard
        rect = self.rect()
        cell = 12
        c1, c2 = QColor(220, 220, 220), QColor(255, 255, 255)
        for y in range(0, rect.height(), cell):
            for x in range(0, rect.width(), cell):
                painter.fillRect(
                    x,
                    y,
                    cell,
                    cell,
                    c1 if (x // cell + y // cell) % 2 == 0 else c2,
                )
        # Draw pixmap centred
        pix = self.pixmap()
        if pix and not pix.isNull():
            px = (rect.width() - pix.width()) // 2
            py = (rect.height() - pix.height()) // 2
            painter.drawPixmap(px, py, pix)
        painter.end()


class BitmapDetailDialog(QDialog):
    """Full-size bitmap detail view with zoom and metadata."""

    ZOOM_LEVELS = [0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0]

    def __init__(self, item: MediaItem, parent=None):
        super().__init__(parent)
        self.item = item
        self._pil = item.pil_image
        self._zoom = 1.0
        self._fit_mode = True

        m = item.member
        title = f"#{item.slot} {m.name or '(unnamed)'} — {m.image_width}×{m.image_height} @ {m.image_bit_depth}bpp"
        self.setWindowTitle(title)
        self.resize(1024, 720)

        # --- Layout ---
        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        # Toolbar
        tb = QWidget()
        tb.setStyleSheet("background:#fff; border-bottom:1px solid #ccc;")
        tb_lay = QHBoxLayout(tb)
        tb_lay.setContentsMargins(8, 4, 8, 4)

        self._zoom_label = QLabel("Fit")
        self._zoom_label.setMinimumWidth(60)

        btn_fit = QPushButton("Fit")
        btn_fit.setToolTip("Fit image to window (0)")
        btn_fit.clicked.connect(self._zoom_fit)

        btn_actual = QPushButton("1:1")
        btn_actual.setToolTip("Actual size (1)")
        btn_actual.clicked.connect(self._zoom_actual)

        btn_in = QPushButton("+")
        btn_in.setToolTip("Zoom in (+)")
        btn_in.setFixedWidth(32)
        btn_in.clicked.connect(self._zoom_in)

        btn_out = QPushButton("−")
        btn_out.setToolTip("Zoom out (−)")
        btn_out.setFixedWidth(32)
        btn_out.clicked.connect(self._zoom_out)

        btn_copy = QPushButton("Copy Image")
        btn_copy.setToolTip("Copy full-size image to clipboard (Ctrl+C)")
        btn_copy.clicked.connect(self._copy_image)

        for w in (btn_fit, btn_actual, btn_out, btn_in, self._zoom_label, btn_copy):
            tb_lay.addWidget(w)
        tb_lay.addStretch()

        # Dimensions info in toolbar
        dim_label = QLabel(
            f"{m.image_width} × {m.image_height}  |  {m.image_bit_depth}bpp  |  "
            f"Palette {m.image_palette}  |  Reg ({m.image_reg_x}, {m.image_reg_y})"
        )
        dim_label.setStyleSheet("color: #666; font-size: 11px;")
        tb_lay.addWidget(dim_label)

        root.addWidget(tb)

        # Splitter: image area + metadata panel
        splitter = QSplitter(Qt.Orientation.Horizontal)

        # -- Image scroll area --
        self._scroll = QScrollArea()
        self._scroll.setWidgetResizable(True)
        self._scroll.setStyleSheet("background: #d0d0d0;")

        self._img_label = _CheckerboardLabel()
        self._img_label.setSizePolicy(
            QSizePolicy.Policy.Ignored, QSizePolicy.Policy.Ignored
        )
        self._scroll.setWidget(self._img_label)

        # -- Metadata panel --
        meta_panel = QTextEdit()
        meta_panel.setReadOnly(True)
        meta_panel.setMaximumWidth(280)
        meta_panel.setMinimumWidth(180)
        meta_panel.setStyleSheet(
            "background:#fafafa; font-family:'Consolas','Courier New',monospace; "
            "font-size:11px; border-left:1px solid #ccc;"
        )
        meta_panel.setPlainText(self._build_metadata())

        splitter.addWidget(self._scroll)
        splitter.addWidget(meta_panel)
        splitter.setStretchFactor(0, 1)
        splitter.setStretchFactor(1, 0)
        splitter.setSizes([750, 274])

        root.addWidget(splitter)

        # Build the full-res QPixmap once
        self._full_pixmap = self._build_full_pixmap()

        # Keyboard shortcuts
        QShortcut(QKeySequence("0"), self, self._zoom_fit)
        QShortcut(QKeySequence("1"), self, self._zoom_actual)
        QShortcut(QKeySequence("+"), self, self._zoom_in)
        QShortcut(QKeySequence("="), self, self._zoom_in)
        QShortcut(QKeySequence("-"), self, self._zoom_out)
        QShortcut(QKeySequence("Ctrl+C"), self, self._copy_image)

        # Initial render
        QTimer.singleShot(0, self._zoom_fit)

    # -- Pixmap ----------------------------------------------------------------

    def _build_full_pixmap(self) -> QPixmap:
        """Convert the PIL image to a full-res QPixmap."""
        pil = self._pil
        if pil is None:
            return QPixmap()
        if pil.mode == "P":
            img = pil.convert("RGBA")
        elif pil.mode == "1":
            img = pil.convert("RGBA")
        elif pil.mode != "RGBA":
            img = pil.convert("RGBA")
        else:
            img = pil
        data = img.tobytes("raw", "RGBA")
        qimg = QImage(
            data, img.width, img.height, 4 * img.width, QImage.Format_RGBA8888
        )
        return QPixmap.fromImage(qimg.copy())

    # -- Zoom ------------------------------------------------------------------

    def _apply_zoom(self):
        if self._full_pixmap.isNull():
            return
        if self._fit_mode:
            # Scale to fit the scroll area viewport
            viewport = self._scroll.viewport()
            assert viewport is not None
            vp = viewport.size()
            scaled = self._full_pixmap.scaled(
                vp.width() - 4,
                vp.height() - 4,
                Qt.AspectRatioMode.KeepAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )
            self._img_label.setPixmap(scaled)
            self._img_label.resize(vp.width(), vp.height())
            # Calculate effective zoom for label
            if self._full_pixmap.width() > 0:
                eff = scaled.width() / self._full_pixmap.width()
                self._zoom_label.setText(f"{eff:.0%}")
            else:
                self._zoom_label.setText("Fit")
        else:
            w = int(self._full_pixmap.width() * self._zoom)
            h = int(self._full_pixmap.height() * self._zoom)
            if w < 1 or h < 1:
                return
            scaled = self._full_pixmap.scaled(
                w,
                h,
                Qt.AspectRatioMode.KeepAspectRatio,
                Qt.TransformationMode.SmoothTransformation,
            )
            self._img_label.setPixmap(scaled)
            self._img_label.resize(scaled.size())
            self._zoom_label.setText(f"{self._zoom:.0%}")

    def _zoom_fit(self):
        self._fit_mode = True
        self._scroll.setWidgetResizable(True)
        self._apply_zoom()

    def _zoom_actual(self):
        self._fit_mode = False
        self._zoom = 1.0
        self._scroll.setWidgetResizable(False)
        self._apply_zoom()

    def _zoom_in(self):
        self._fit_mode = False
        self._scroll.setWidgetResizable(False)
        # Find next higher zoom level
        for z in self.ZOOM_LEVELS:
            if z > self._zoom + 0.001:
                self._zoom = z
                break
        self._apply_zoom()

    def _zoom_out(self):
        self._fit_mode = False
        self._scroll.setWidgetResizable(False)
        # Find next lower zoom level
        for z in reversed(self.ZOOM_LEVELS):
            if z < self._zoom - 0.001:
                self._zoom = z
                break
        self._apply_zoom()

    def resizeEvent(self, a0):  # type: ignore[override]
        super().resizeEvent(a0)
        if self._fit_mode:
            self._apply_zoom()

    # -- Actions ---------------------------------------------------------------

    def _copy_image(self):
        if self._full_pixmap.isNull():
            return
        cb = QApplication.clipboard()
        if cb:
            cb.setPixmap(self._full_pixmap)
        parent = self.parent()
        if isinstance(parent, QMainWindow):
            sb = parent.statusBar()
            if sb is not None:
                sb.showMessage(
                    f"Copied bitmap #{self.item.slot} to clipboard",
                    3000,
                )

    def _build_metadata(self) -> str:
        m = self.item.member
        lines = [
            f"Slot:        #{self.item.slot}",
            f"Name:        {m.name or '(unnamed)'}",
            f"Type:        {m.type_name}",
            f"Source:      {self.item.source_file}",
            f"Library:     {self.item.lib_name}",
            "",
            "=== Bitmap ===",
            f"Width:       {m.image_width}",
            f"Height:      {m.image_height}",
            f"Bit depth:   {m.image_bit_depth}bpp",
            f"Palette ID:  {m.image_palette}",
            f"Reg point:   ({m.image_reg_x}, {m.image_reg_y})",
            "",
            "=== Internal ===",
            f"CastMember slot: {m.slot}",
            f"File slot:   {m.file_slot}",
            f"Linked:      {m.linked_entries}",
        ]
        if m.shape_data:
            lines.append(f"Shape data:  {m.shape_data}")
        return "\n".join(lines)


# ---------------------------------------------------------------------------
# Card Widget — one cell in the grid
# ---------------------------------------------------------------------------


class MediaCard(QFrame):
    """A clickable card showing one cast member."""

    def __init__(self, item: MediaItem, parent=None):
        super().__init__(parent)
        self.item = item
        self._selected = False
        self._playing = False

        self.setFrameStyle(QFrame.Box | QFrame.Plain)
        self.setLineWidth(1)
        self.setFixedSize(THUMB_SIZE + 8, THUMB_SIZE + 44)
        self.setCursor(Qt.CursorShape.PointingHandCursor)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(CARD_PAD, CARD_PAD, CARD_PAD, CARD_PAD)
        layout.setSpacing(1)

        # Thumbnail
        self.thumb_label = QLabel()
        self.thumb_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.thumb_label.setFixedSize(THUMB_SIZE + 2, THUMB_SIZE + 2)
        if item.thumbnail:
            self.thumb_label.setPixmap(item.thumbnail)
        layout.addWidget(self.thumb_label)

        # ID + Name label
        name = item.member.name or "(unnamed)"
        type_name = CAST_TYPE_NAMES.get(item.member.cast_type, "?")
        colour = TYPE_COLOURS.get(item.member.cast_type, "#757575")
        id_text = f"<b style='color:{colour}'>[{type_name}]</b> #{item.slot}"
        name_text = f"<small>{_elide(name, 24)}</small>"

        info_label = QLabel(f"{id_text}<br>{name_text}")
        info_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        info_label.setWordWrap(True)
        info_label.setTextFormat(Qt.TextFormat.RichText)
        info_label.setStyleSheet("font-size: 10px;")
        layout.addWidget(info_label)

        # Tooltip with full details
        self.setToolTip(self._build_tooltip())
        self._update_style()

    # -- Info helpers ----------------------------------------------------------

    def _build_tooltip(self) -> str:
        m = self.item.member
        lines = [
            f"#{self.item.slot}  {m.name or '(unnamed)'}",
            f"Type: {m.type_name}",
            f"Source: {self.item.source_file}",
            f"Library: {self.item.lib_name}",
        ]
        if m.cast_type == CastType.BITMAP:
            lines.append(
                f"Size: {m.image_width}\u00d7{m.image_height} @ {m.image_bit_depth}bpp"
            )
            lines.append(f"Reg: ({m.image_reg_x}, {m.image_reg_y})")
            lines.append(f"Palette ID: {m.image_palette}")
        elif m.cast_type == CastType.SOUND:
            lines.append(f"Duration: {m.sound_duration_seconds:.2f}s")
            lines.append(
                f"Rate: {m.sound_sample_rate}Hz {m.sound_sample_size}bit "
                f"{m.sound_channels}ch"
            )
            if self.item.wav_data:
                lines.append("Click to play / pause")
        if self.item.description:
            lines.append(f"Info: {self.item.description}")
        return "\n".join(lines)

    def _get_impl_info(self) -> str:
        """Build implementation-ready info string for clipboard."""
        m = self.item.member
        lines = [
            f'// Cast Member #{self.item.slot} "{m.name}"',
            f"// Source: {self.item.source_file}, Library: {self.item.lib_name}",
            f"// Type: {m.type_name} (cast_type={m.cast_type})",
        ]
        if m.cast_type == CastType.BITMAP:
            lines += [
                f"// Size: {m.image_width}x{m.image_height}, "
                f"Depth: {m.image_bit_depth}bpp",
                f"// RegPoint: ({m.image_reg_x}, {m.image_reg_y})",
                f"// Palette: {m.image_palette}",
                f"// Slot: {m.slot}, FileSlot: {m.file_slot}",
                f"// LinkedEntries: {m.linked_entries}",
                f'find_member_by_name("{m.name}")',
            ]
        elif m.cast_type == CastType.SOUND:
            lines += [
                f"// Rate: {m.sound_sample_rate}Hz, "
                f"Size: {m.sound_sample_size}bit, Ch: {m.sound_channels}",
                f"// Duration: {m.sound_duration_seconds:.2f}s, "
                f"Looped: {m.sound_looped}",
                f'find_member_by_name("{m.name}")',
            ]
        elif m.cast_type == CastType.SHAPE:
            lines += [f"// ShapeData: {m.shape_data}"]
        else:
            if m.name:
                lines.append(f'find_member_by_name("{m.name}")')
        return "\n".join(lines)

    # -- Visual state ----------------------------------------------------------

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

    def set_playing(self, playing: bool):
        """Update visual state for sound playback."""
        self._playing = playing
        if self.item.member.cast_type == CastType.SOUND:
            m = self.item.member
            dur = m.sound_duration_seconds
            colour = TYPE_COLOURS.get(CastType.SOUND, "#2196F3")
            if playing:
                text = f"\u23f8 Playing\n{dur:.1f}s\n{m.sound_sample_rate}Hz"
            else:
                text = f"\u25b6 Sound\n{dur:.1f}s\n{m.sound_sample_rate}Hz"
            self.thumb_label.setPixmap(_make_placeholder(text, colour, THUMB_SIZE))

    # -- Events ----------------------------------------------------------------

    def _find_grid(self) -> "GridPanel | None":
        parent = self.parent()
        while parent and not isinstance(parent, GridPanel):
            parent = parent.parent()
        return parent

    def mousePressEvent(self, a0):  # type: ignore[override]
        if a0 is not None and a0.button() == Qt.MouseButton.LeftButton:
            grid = self._find_grid()
            if grid:
                grid.select_card(self)
                # Toggle sound playback for sound cards
                if self.item.wav_data:
                    grid.toggle_sound(self)
                # Open detail view for bitmap cards
                elif (
                    self.item.member.cast_type == CastType.BITMAP
                    and self.item.pil_image is not None
                ):
                    grid.open_bitmap_detail(self)
        super().mousePressEvent(a0)

    def contextMenuEvent(self, a0):  # type: ignore[override]
        if a0 is None:
            return
        menu = QMenu(self)
        act_impl = menu.addAction("Copy Implementation Info")
        act_img = None
        if self.item.pil_image:
            act_img = menu.addAction("Copy Image to Clipboard")
        act_meta = menu.addAction("Copy Metadata as Text")

        action = menu.exec_(a0.globalPos())
        if not action:
            return

        cb = QApplication.clipboard()
        if cb is None:
            return
        msg = ""

        if action == act_impl:
            cb.setText(self._get_impl_info())
            msg = f"Copied impl info for #{self.item.slot}"

        elif act_img and action == act_img:
            pil = self.item.pil_image
            if pil.mode == "P":
                rgb = pil.convert("RGB")
                data = rgb.tobytes("raw", "RGB")
                qimg = QImage(
                    data,
                    rgb.width,
                    rgb.height,
                    3 * rgb.width,
                    QImage.Format_RGB888,
                )
            else:
                rgba = pil.convert("RGBA")
                data = rgba.tobytes("raw", "RGBA")
                qimg = QImage(
                    data,
                    rgba.width,
                    rgba.height,
                    4 * rgba.width,
                    QImage.Format_RGBA8888,
                )
            cb.setImage(qimg.copy())
            msg = f"Copied bitmap #{self.item.slot} to clipboard"

        elif action == act_meta:
            m = self.item.member
            info = (
                f"ID: {self.item.slot}\n"
                f"Name: {m.name}\n"
                f"Type: {m.type_name}\n"
                f"Library: {self.item.lib_name}\n"
                f"Source: {self.item.source_file}\n"
                f"Description: {self.item.description}"
            )
            cb.setText(info)
            msg = f"Copied metadata for #{self.item.slot}"

        if msg:
            main = self.window()
            if isinstance(main, MainWindow):
                main.statusBar().showMessage(msg, 3000)


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
        self._playing_card: MediaCard | None = None
        self._sound_tmpfile: str | None = None
        self._container = QWidget()
        self._grid = QGridLayout(self._container)
        self._grid.setSpacing(GRID_SPACING)
        self._grid.setContentsMargins(4, 4, 4, 4)
        self.setWidget(self._container)

        # Debounce timer for resize relayout
        self._resize_timer = QTimer(self)
        self._resize_timer.setSingleShot(True)
        self._resize_timer.setInterval(150)
        self._resize_timer.timeout.connect(self._relayout)

    def _calc_cols(self) -> int:
        vp = self.viewport()
        w = vp.width() if vp else 400
        return max(1, (w - 8) // (THUMB_SIZE + GRID_SPACING + 8))

    def set_items(self, items: list[MediaItem]):
        """Populate the grid with items."""
        # Stop any playing sound
        self._stop_sound()
        # Clear old
        for card in self._cards:
            card.setParent(None)
            card.deleteLater()
        self._cards.clear()
        self._selected = None

        cols = self._calc_cols()

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
        cb = QApplication.clipboard()
        if cb is None:
            return

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
            cb.setImage(qimg.copy())
            main = self.window()
            if isinstance(main, MainWindow):
                main.statusBar().showMessage(
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
            cb.setText(info)
            main = self.window()
            if isinstance(main, MainWindow):
                main.statusBar().showMessage(
                    f"Copied info for #{item.slot} '{item.member.name}' to clipboard",
                    3000,
                )

    def resizeEvent(self, a0):  # type: ignore[override]
        super().resizeEvent(a0)
        # Debounce relayout to avoid expensive per-pixel re-gridding
        self._resize_timer.start()

    def _relayout(self):
        """Re-grid all cards to match current viewport width."""
        if not self._cards:
            return
        cols = self._calc_cols()
        for i, card in enumerate(self._cards):
            self._grid.removeWidget(card)
            self._grid.addWidget(card, i // cols, i % cols)

    # -- Sound playback --------------------------------------------------------

    def _stop_sound(self):
        """Stop any currently playing sound."""
        if self._playing_card:
            winsound.PlaySound(None, winsound.SND_PURGE)
            self._playing_card.set_playing(False)
            self._playing_card = None
        if self._sound_tmpfile:
            try:
                os.unlink(self._sound_tmpfile)
            except OSError:
                pass
            self._sound_tmpfile = None

    def toggle_sound(self, card: MediaCard):
        """Toggle sound playback for a card."""
        if self._playing_card is card:
            # Stop current
            self._stop_sound()
        else:
            # Stop previous if any, then play new
            self._stop_sound()
            if card.item.wav_data:
                try:
                    fd, tmp_path = tempfile.mkstemp(suffix=".wav")
                    os.write(fd, card.item.wav_data)
                    os.close(fd)
                    self._sound_tmpfile = tmp_path
                    winsound.PlaySound(
                        tmp_path,
                        winsound.SND_FILENAME | winsound.SND_ASYNC,
                    )
                    card.set_playing(True)
                    self._playing_card = card
                except Exception as e:
                    log.warning("Sound playback failed: %s", e)

    def open_bitmap_detail(self, card: MediaCard):
        """Open the bitmap detail dialog for a card."""
        main = self.window()
        dlg = BitmapDetailDialog(card.item, parent=main)
        dlg.exec_()


# ---------------------------------------------------------------------------
# Main Window
# ---------------------------------------------------------------------------


class MainWindow(QMainWindow):
    # -- helpers ----------------------------------------------------------
    def _status(self, msg: str, timeout: int = 0) -> None:
        """Safely show a status bar message (avoids Pylance Optional warning)."""
        sb = self.statusBar()
        if sb is not None:
            sb.showMessage(msg, timeout)

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
        self._status(f"Found {len(self._dir_files)} Director files in {game_dir}")

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
            self._status(f"Loading {path.name}…")
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
        self._status(f"Loaded {path.name}: {len(self._all_items)} cast members")

    def _load_all_files(self):
        """Load all Director files and merge items."""
        progress = QProgressDialog(
            "Loading Director files…", "Cancel", 0, len(self._dir_files), self
        )
        progress.setWindowModality(Qt.WindowModality.WindowModal)
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
        self._status(f"Loaded all files: {len(self._all_items)} cast members total")

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
