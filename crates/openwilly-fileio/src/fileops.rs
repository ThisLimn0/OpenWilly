//! File operations implementation
//!
//! Implements the actual file I/O operations for the FILEIO Xtra.
//! Each method corresponds to a Lingo command.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::debug_log;
use crate::pathmap;

// ============================================================================
// Error Codes (matching original FILEIO.X32)
// ============================================================================

pub const ERR_OK: i32 = 0;
pub const ERR_MEM_ALLOC: i32 = 1;
pub const ERR_FILE_DIR_FULL: i32 = -33;
pub const ERR_VOLUME_FULL: i32 = -34;
pub const ERR_VOLUME_NOT_FOUND: i32 = -35;
pub const ERR_IO_ERROR: i32 = -36;
pub const ERR_BAD_FILE_NAME: i32 = -37;
pub const ERR_FILE_NOT_OPEN: i32 = -38;
pub const ERR_TOO_MANY_FILES: i32 = -42;
pub const ERR_FILE_NOT_FOUND: i32 = -43;
pub const ERR_NO_SUCH_DRIVE: i32 = -56;
pub const ERR_NO_DISK: i32 = -65;
pub const ERR_DIR_NOT_FOUND: i32 = -120;
pub const ERR_FILE_HAS_OPEN: i32 = -121;
pub const ERR_FILE_EXISTS: i32 = -122;
pub const ERR_READ_ONLY: i32 = -123;
pub const ERR_WRITE_ONLY: i32 = -124;

/// Convert an error code to its string representation
pub fn error_string(code: i32) -> &'static str {
    match code {
        ERR_OK => "OK",
        ERR_MEM_ALLOC => "Memory allocation failure",
        ERR_FILE_DIR_FULL => "File directory full",
        ERR_VOLUME_FULL => "Volume full",
        ERR_VOLUME_NOT_FOUND => "Volume not found",
        ERR_IO_ERROR => "I/O Error",
        ERR_BAD_FILE_NAME => "Bad file name",
        ERR_FILE_NOT_OPEN => "File not open",
        ERR_TOO_MANY_FILES => "Too many files open",
        ERR_FILE_NOT_FOUND => "File not found",
        ERR_NO_SUCH_DRIVE => "No such drive",
        ERR_NO_DISK => "No disk in drive",
        ERR_DIR_NOT_FOUND => "Directory not found",
        ERR_FILE_HAS_OPEN => "Instance has an open file",
        ERR_FILE_EXISTS => "File already exists",
        ERR_READ_ONLY => "File is opened read-only",
        ERR_WRITE_ONLY => "File is opened write-only",
        _ => "Unknown error",
    }
}

// ============================================================================
// File Open Modes
// ============================================================================

/// File open mode (matches original Lingo interface)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenMode {
    ReadWrite = 0,
    ReadOnly = 1,
    WriteOnly = 2,
}

impl OpenMode {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(OpenMode::ReadWrite),
            1 => Some(OpenMode::ReadOnly),
            2 => Some(OpenMode::WriteOnly),
            _ => None,
        }
    }
}

// ============================================================================
// FileIO Instance – per-Xtra-object state
// ============================================================================

/// Represents one FileIO Xtra instance (created per `new(xtra "fileio")`)
pub struct FileIOInstance {
    /// Current file name (empty if no file open)
    pub file_name: String,
    /// Currently open file handle
    pub file: Option<File>,
    /// Current open mode (-1 = closed)
    pub open_mode: i32,
    /// Last error status
    pub status: i32,
    /// Filter mask for open/save dialogs
    pub filter_mask: String,
}

impl FileIOInstance {
    /// Create a new FileIO instance (handler 0: `new`)
    pub fn new() -> Self {
        debug_log("FileIO: new instance created");
        Self {
            file_name: String::new(),
            file: None,
            open_mode: -1,
            status: ERR_OK,
            filter_mask: "All Files\0*.*\0".to_string(),
        }
    }

    /// Handler 1: `fileName` – return the current file name
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Handler 2: `status` – return the last error code
    pub fn status(&self) -> i32 {
        self.status
    }

    /// Handler 3: `error` – return error string for a given code
    pub fn error(&self, code: i32) -> &'static str {
        error_string(code)
    }

    /// Handler 4: `setFilterMask` – set the filter mask for dialogs
    pub fn set_filter_mask(&mut self, mask: &str) {
        debug_log(&format!("FileIO: setFilterMask(\"{}\")", mask));
        // Replace commas with null bytes (original behaviour)
        self.filter_mask = mask.replace(',', "\0");
        self.status = ERR_OK;
    }

    /// Handler 5: `openFile` – open a file with the given mode
    /// mode: 0=read/write, 1=read-only, 2=write-only
    pub fn open_file(&mut self, file_name: &str, mode: i32) {
        debug_log(&format!("FileIO: openFile(\"{}\", {})", file_name, mode));

        if self.file.is_some() {
            self.status = ERR_FILE_HAS_OPEN;
            return;
        }

        let mode = match OpenMode::from_i32(mode) {
            Some(m) => m,
            None => {
                self.status = ERR_BAD_FILE_NAME;
                return;
            }
        };

        // Apply CD-path bypass
        let actual_path = pathmap::redirect_path(file_name);
        debug_log(&format!("FileIO: resolved path: \"{}\"", actual_path));

        let result = match mode {
            OpenMode::ReadWrite => OpenOptions::new()
                .read(true)
                .write(true)
                .open(&actual_path),
            OpenMode::ReadOnly => OpenOptions::new()
                .read(true)
                .open(&actual_path),
            OpenMode::WriteOnly => OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&actual_path),
        };

        match result {
            Ok(file) => {
                self.file = Some(file);
                self.file_name = file_name.to_string();
                self.open_mode = mode as i32;
                self.status = ERR_OK;
                debug_log(&format!("FileIO: file opened successfully"));
            }
            Err(e) => {
                self.status = io_error_to_code(&e);
                debug_log(&format!("FileIO: open failed: {} (code {})", e, self.status));
            }
        }
    }

    /// Handler 6: `closeFile` – close the current file
    pub fn close_file(&mut self) {
        debug_log("FileIO: closeFile");
        if self.file.is_some() {
            self.file = None;
            self.open_mode = -1;
            self.status = ERR_OK;
        }
        // Not an error if no file was open (original behavior)
    }

    /// Handler 7: `displayOpen` – show an Open File dialog
    pub fn display_open(&mut self) -> String {
        debug_log("FileIO: displayOpen");
        // TODO: Implement using GetOpenFileNameA
        // For now return empty string
        self.status = ERR_OK;
        String::new()
    }

    /// Handler 8: `displaySave` – show a Save File dialog
    pub fn display_save(&mut self, _title: &str, _default_name: &str) -> String {
        debug_log("FileIO: displaySave");
        // TODO: Implement using GetSaveFileNameA
        self.status = ERR_OK;
        String::new()
    }

    /// Handler 9: `createFile` – create a new file
    pub fn create_file(&mut self, file_name: &str) {
        debug_log(&format!("FileIO: createFile(\"{}\")", file_name));

        if self.file.is_some() {
            self.status = ERR_FILE_HAS_OPEN;
            return;
        }

        let actual_path = pathmap::redirect_path(file_name);

        // Check if file already exists
        if PathBuf::from(&actual_path).exists() {
            self.status = ERR_FILE_EXISTS;
            return;
        }

        match File::create(&actual_path) {
            Ok(file) => {
                self.file = Some(file);
                self.file_name = file_name.to_string();
                self.open_mode = OpenMode::ReadWrite as i32;
                self.status = ERR_OK;
            }
            Err(e) => {
                self.status = io_error_to_code(&e);
            }
        }
    }

    /// Handler 10: `setPosition` – seek to position in file
    pub fn set_position(&mut self, position: i32) {
        if let Some(ref mut file) = self.file {
            match file.seek(SeekFrom::Start(position as u64)) {
                Ok(_) => self.status = ERR_OK,
                Err(_) => self.status = ERR_IO_ERROR,
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
        }
    }

    /// Handler 11: `getPosition` – get current position in file
    pub fn get_position(&mut self) -> i32 {
        if let Some(ref mut file) = self.file {
            match file.stream_position() {
                Ok(pos) => {
                    self.status = ERR_OK;
                    pos as i32
                }
                Err(_) => {
                    self.status = ERR_IO_ERROR;
                    0
                }
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
            0
        }
    }

    /// Handler 12: `getLength` – get file length
    pub fn get_length(&mut self) -> i32 {
        if let Some(ref file) = self.file {
            match file.metadata() {
                Ok(meta) => {
                    self.status = ERR_OK;
                    meta.len() as i32
                }
                Err(_) => {
                    self.status = ERR_IO_ERROR;
                    0
                }
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
            0
        }
    }

    /// Handler 13: `writeChar` – write a single character by ASCII code
    pub fn write_char(&mut self, char_code: i32) {
        if self.open_mode == OpenMode::ReadOnly as i32 {
            self.status = ERR_READ_ONLY;
            return;
        }
        if let Some(ref mut file) = self.file {
            let byte = char_code as u8;
            match file.write_all(&[byte]) {
                Ok(_) => self.status = ERR_OK,
                Err(_) => self.status = ERR_IO_ERROR,
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
        }
    }

    /// Handler 14: `writeString` – write a null-terminated string
    pub fn write_string(&mut self, s: &str) {
        if self.open_mode == OpenMode::ReadOnly as i32 {
            self.status = ERR_READ_ONLY;
            return;
        }
        if let Some(ref mut file) = self.file {
            match file.write_all(s.as_bytes()) {
                Ok(_) => self.status = ERR_OK,
                Err(_) => self.status = ERR_IO_ERROR,
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
        }
    }

    /// Handler 15: `readChar` – read the next character as ASCII code
    pub fn read_char(&mut self) -> i32 {
        if self.open_mode == OpenMode::WriteOnly as i32 {
            self.status = ERR_WRITE_ONLY;
            return -1;
        }
        if let Some(ref mut file) = self.file {
            let mut buf = [0u8; 1];
            match file.read(&mut buf) {
                Ok(0) => -1, // EOF
                Ok(_) => {
                    self.status = ERR_OK;
                    buf[0] as i32
                }
                Err(_) => {
                    self.status = ERR_IO_ERROR;
                    -1
                }
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
            -1
        }
    }

    /// Handler 16: `readLine` – read until next newline (including the newline)
    pub fn read_line(&mut self) -> String {
        if self.open_mode == OpenMode::WriteOnly as i32 {
            self.status = ERR_WRITE_ONLY;
            return String::new();
        }
        if let Some(ref mut file) = self.file {
            let mut reader = io::BufReader::new(file);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF
                    self.status = ERR_OK;
                    String::new()
                }
                Ok(_) => {
                    self.status = ERR_OK;
                    // Seek the underlying file to the right position
                    // (BufReader may have read more than needed)
                    let _consumed = line.len() as i64;
                    let buffered = reader.buffer().len() as i64;
                    if let Ok(inner) = reader.into_inner().seek(SeekFrom::Current(-buffered)) {
                        let _ = inner;
                    }
                    line
                }
                Err(_) => {
                    self.status = ERR_IO_ERROR;
                    String::new()
                }
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
            String::new()
        }
    }

    /// Handler 17: `readFile` – read from current position to EOF
    pub fn read_file(&mut self) -> String {
        if self.open_mode == OpenMode::WriteOnly as i32 {
            self.status = ERR_WRITE_ONLY;
            return String::new();
        }
        if let Some(ref mut file) = self.file {
            let mut contents = String::new();
            match file.read_to_string(&mut contents) {
                Ok(_) => {
                    self.status = ERR_OK;
                    contents
                }
                Err(_) => {
                    // Try reading as lossy UTF-8
                    let mut buf = Vec::new();
                    if let Ok(pos) = file.stream_position() {
                        let _ = file.seek(SeekFrom::Start(pos));
                    }
                    match file.read_to_end(&mut buf) {
                        Ok(_) => {
                            self.status = ERR_OK;
                            String::from_utf8_lossy(&buf).to_string()
                        }
                        Err(_) => {
                            self.status = ERR_IO_ERROR;
                            String::new()
                        }
                    }
                }
            }
        } else {
            self.status = ERR_FILE_NOT_OPEN;
            String::new()
        }
    }

    /// Handler 18: `readToken` – read next token using skip/break chars
    pub fn read_token(&mut self, skip: &str, break_chars: &str) -> String {
        if self.open_mode == OpenMode::WriteOnly as i32 {
            self.status = ERR_WRITE_ONLY;
            return String::new();
        }
        if let Some(ref mut file) = self.file {
            let mut result = String::new();

            // Skip characters in the `skip` set
            loop {
                let mut buf = [0u8; 1];
                match file.read(&mut buf) {
                    Ok(0) => {
                        self.status = ERR_OK;
                        return result;
                    }
                    Ok(_) => {
                        let ch = buf[0] as char;
                        if !skip.contains(ch) {
                            // Check if it's a break character
                            if break_chars.contains(ch) {
                                self.status = ERR_OK;
                                return result;
                            }
                            result.push(ch);
                            break;
                        }
                    }
                    Err(_) => {
                        self.status = ERR_IO_ERROR;
                        return String::new();
                    }
                }
            }

            // Read until break character or EOF
            loop {
                let mut buf = [0u8; 1];
                match file.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let ch = buf[0] as char;
                        if break_chars.contains(ch) {
                            break;
                        }
                        result.push(ch);
                    }
                    Err(_) => {
                        self.status = ERR_IO_ERROR;
                        return String::new();
                    }
                }
            }

            self.status = ERR_OK;
            result
        } else {
            self.status = ERR_FILE_NOT_OPEN;
            String::new()
        }
    }

    /// Handler 19: `readWord` – read the next whitespace-delimited word
    pub fn read_word(&mut self) -> String {
        self.read_token(" \t\r\n", " \t\r\n")
    }

    /// Handler 20: `getFinderInfo` – Mac only, stub
    pub fn get_finder_info(&self) -> i32 {
        0
    }

    /// Handler 21: `setFinderInfo` – Mac only, stub
    pub fn set_finder_info(&self, _attrs: &str) {
        // No-op on Windows
    }

    /// Handler 22: `delete` – delete the open file
    pub fn delete(&mut self) {
        debug_log("FileIO: delete");
        let name = self.file_name.clone();
        self.close_file();

        if !name.is_empty() {
            let actual_path = pathmap::redirect_path(&name);
            match fs::remove_file(&actual_path) {
                Ok(_) => {
                    self.status = ERR_OK;
                    self.file_name.clear();
                }
                Err(e) => {
                    self.status = io_error_to_code(&e);
                }
            }
        }
    }

    /// Handler 23: `version` – return version string
    pub fn version(&self) -> String {
        "FileIO 1.0 (OpenWilly)".to_string()
    }

    /// Handler 24: `getOSDirectory` – return the Windows directory
    pub fn get_os_directory(&self) -> String {
        std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string())
    }
}

impl Default for FileIOInstance {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a std::io::Error to a FileIO error code
fn io_error_to_code(e: &io::Error) -> i32 {
    match e.kind() {
        io::ErrorKind::NotFound => ERR_FILE_NOT_FOUND,
        io::ErrorKind::PermissionDenied => ERR_READ_ONLY,
        io::ErrorKind::AlreadyExists => ERR_FILE_EXISTS,
        _ => ERR_IO_ERROR,
    }
}
