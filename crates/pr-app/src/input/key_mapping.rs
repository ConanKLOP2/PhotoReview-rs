//! Port of `PhotoReview.App.Shared/Input/KeyIdMapping.cs` (C-06, 419 lines) at baseline `d92d6aae`, and of the name table
//! that `Enum.TryParse` reads from `KeyId.cs` (202 field names, aliases included). Target: `pr_app::input::key_mapping`
//! (W8-05c, docs/WAVE-8-PLAN.md). Pure: no `windows` crate, no `unsafe`, no clock, no I/O (AGENTS.md, WAVE-8-PLAN layering).
//!
//! Stub: the three tables are final data (generated from the C# source at `d92d6aae`, one row per C# entry, C# order, with the
//! C# name in a trailing comment). The three public functions are `todo!()`. The porter fills bodies only, and must not edit
//! the tables (a wrong row is fixed by regenerating from the C# file; see contract.md "Golden package").
//! `KeyId` comes from W8-05b (`input::key_id`); see contract.md "Depends on" and open question 1.

#![allow(unused_variables, dead_code)] // stub: removed when the port lands (contracts/README.md D11)

use crate::input::key_id::KeyId;

/// `KeyIdMapping.VirtualKeyToKey` (C# lines 63-234): `(virtual-key code, KeyId value)`, C# order, 172 rows.
/// Codes `0x10`, `0x11`, `0x12` (generic Shift, Control, Menu) map to the LEFT keys; `0xA0`..`0xA5` map to LEFT and RIGHT keys.
/// A code without a row here maps to `KeyId::None` (the porter checks the exact set with the golden, not by hand).
pub(crate) const VIRTUAL_KEY_TO_KEY: &[(i32, i32)] = &[
    (0x03, 1),   // Cancel
    (0x08, 2),   // Back
    (0x09, 3),   // Tab
    (0x0C, 5),   // Clear
    (0x0D, 6),   // Return
    (0x10, 116), // LeftShift
    (0x11, 118), // LeftCtrl
    (0x12, 120), // LeftAlt
    (0x13, 7),   // Pause
    (0x14, 8),   // Capital
    (0x15, 9),   // KanaMode
    (0x17, 10),  // JunjaMode
    (0x18, 11),  // FinalMode
    (0x19, 12),  // HanjaMode
    (0x1B, 13),  // Escape
    (0x1C, 14),  // ImeConvert
    (0x1D, 15),  // ImeNonConvert
    (0x1E, 16),  // ImeAccept
    (0x1F, 17),  // ImeModeChange
    (0x20, 18),  // Space
    (0x21, 19),  // Prior
    (0x22, 20),  // Next
    (0x23, 21),  // End
    (0x24, 22),  // Home
    (0x25, 23),  // Left
    (0x26, 24),  // Up
    (0x27, 25),  // Right
    (0x28, 26),  // Down
    (0x29, 27),  // Select
    (0x2A, 28),  // Print
    (0x2B, 29),  // Execute
    (0x2C, 30),  // Snapshot
    (0x2D, 31),  // Insert
    (0x2E, 32),  // Delete
    (0x2F, 33),  // Help
    (0x30, 34),  // D0
    (0x31, 35),  // D1
    (0x32, 36),  // D2
    (0x33, 37),  // D3
    (0x34, 38),  // D4
    (0x35, 39),  // D5
    (0x36, 40),  // D6
    (0x37, 41),  // D7
    (0x38, 42),  // D8
    (0x39, 43),  // D9
    (0x41, 44),  // A
    (0x42, 45),  // B
    (0x43, 46),  // C
    (0x44, 47),  // D
    (0x45, 48),  // E
    (0x46, 49),  // F
    (0x47, 50),  // G
    (0x48, 51),  // H
    (0x49, 52),  // I
    (0x4A, 53),  // J
    (0x4B, 54),  // K
    (0x4C, 55),  // L
    (0x4D, 56),  // M
    (0x4E, 57),  // N
    (0x4F, 58),  // O
    (0x50, 59),  // P
    (0x51, 60),  // Q
    (0x52, 61),  // R
    (0x53, 62),  // S
    (0x54, 63),  // T
    (0x55, 64),  // U
    (0x56, 65),  // V
    (0x57, 66),  // W
    (0x58, 67),  // X
    (0x59, 68),  // Y
    (0x5A, 69),  // Z
    (0x5B, 70),  // LWin
    (0x5C, 71),  // RWin
    (0x5D, 72),  // Apps
    (0x5F, 73),  // Sleep
    (0x60, 74),  // NumPad0
    (0x61, 75),  // NumPad1
    (0x62, 76),  // NumPad2
    (0x63, 77),  // NumPad3
    (0x64, 78),  // NumPad4
    (0x65, 79),  // NumPad5
    (0x66, 80),  // NumPad6
    (0x67, 81),  // NumPad7
    (0x68, 82),  // NumPad8
    (0x69, 83),  // NumPad9
    (0x6A, 84),  // Multiply
    (0x6B, 85),  // Add
    (0x6C, 86),  // Separator
    (0x6D, 87),  // Subtract
    (0x6E, 88),  // Decimal
    (0x6F, 89),  // Divide
    (0x70, 90),  // F1
    (0x71, 91),  // F2
    (0x72, 92),  // F3
    (0x73, 93),  // F4
    (0x74, 94),  // F5
    (0x75, 95),  // F6
    (0x76, 96),  // F7
    (0x77, 97),  // F8
    (0x78, 98),  // F9
    (0x79, 99),  // F10
    (0x7A, 100), // F11
    (0x7B, 101), // F12
    (0x7C, 102), // F13
    (0x7D, 103), // F14
    (0x7E, 104), // F15
    (0x7F, 105), // F16
    (0x80, 106), // F17
    (0x81, 107), // F18
    (0x82, 108), // F19
    (0x83, 109), // F20
    (0x84, 110), // F21
    (0x85, 111), // F22
    (0x86, 112), // F23
    (0x87, 113), // F24
    (0x90, 114), // NumLock
    (0x91, 115), // Scroll
    (0xA0, 116), // LeftShift
    (0xA1, 117), // RightShift
    (0xA2, 118), // LeftCtrl
    (0xA3, 119), // RightCtrl
    (0xA4, 120), // LeftAlt
    (0xA5, 121), // RightAlt
    (0xA6, 122), // BrowserBack
    (0xA7, 123), // BrowserForward
    (0xA8, 124), // BrowserRefresh
    (0xA9, 125), // BrowserStop
    (0xAA, 126), // BrowserSearch
    (0xAB, 127), // BrowserFavorites
    (0xAC, 128), // BrowserHome
    (0xAD, 129), // VolumeMute
    (0xAE, 130), // VolumeDown
    (0xAF, 131), // VolumeUp
    (0xB0, 132), // MediaNextTrack
    (0xB1, 133), // MediaPreviousTrack
    (0xB2, 134), // MediaStop
    (0xB3, 135), // MediaPlayPause
    (0xB4, 136), // LaunchMail
    (0xB5, 137), // SelectMedia
    (0xB6, 138), // LaunchApplication1
    (0xB7, 139), // LaunchApplication2
    (0xBA, 140), // Oem1
    (0xBB, 141), // OemPlus
    (0xBC, 142), // OemComma
    (0xBD, 143), // OemMinus
    (0xBE, 144), // OemPeriod
    (0xBF, 145), // Oem2
    (0xC0, 146), // Oem3
    (0xC1, 147), // AbntC1
    (0xC2, 148), // AbntC2
    (0xDB, 149), // Oem4
    (0xDC, 150), // Oem5
    (0xDD, 151), // Oem6
    (0xDE, 152), // Oem7
    (0xDF, 153), // Oem8
    (0xE2, 154), // Oem102
    (0xE5, 155), // ImeProcessed
    (0xF0, 157), // OemAttn
    (0xF1, 158), // OemFinish
    (0xF2, 159), // OemCopy
    (0xF3, 160), // OemAuto
    (0xF4, 161), // OemEnlw
    (0xF5, 162), // OemBackTab
    (0xF6, 163), // Attn
    (0xF7, 164), // CrSel
    (0xF8, 165), // ExSel
    (0xF9, 166), // EraseEof
    (0xFA, 167), // Play
    (0xFB, 168), // Zoom
    (0xFC, 169), // NoName
    (0xFD, 170), // Pa1
    (0xFE, 171), // OemClear
];

/// `KeyIdMapping.KeyToVirtualKey` (C# lines 245-413): `(KeyId value, virtual-key code)`, C# order, 169 rows.
/// Built from the reverse dictionary: it has NO row for the generic codes (`0x10`..`0x12`); `LeftShift`/`LeftCtrl`/`LeftAlt`
/// map back to `0xA0`/`0xA2`/`0xA4`. Keys without a code (`None`, `LineFeed`, `System`, `DeadCharProcessed`) have no row.
pub(crate) const KEY_TO_VIRTUAL_KEY: &[(i32, i32)] = &[
    (1, 0x03),   // Cancel
    (2, 0x08),   // Back
    (3, 0x09),   // Tab
    (5, 0x0C),   // Clear
    (6, 0x0D),   // Return
    (7, 0x13),   // Pause
    (8, 0x14),   // Capital
    (9, 0x15),   // KanaMode
    (10, 0x17),  // JunjaMode
    (11, 0x18),  // FinalMode
    (12, 0x19),  // HanjaMode
    (13, 0x1B),  // Escape
    (14, 0x1C),  // ImeConvert
    (15, 0x1D),  // ImeNonConvert
    (16, 0x1E),  // ImeAccept
    (17, 0x1F),  // ImeModeChange
    (18, 0x20),  // Space
    (19, 0x21),  // Prior
    (20, 0x22),  // Next
    (21, 0x23),  // End
    (22, 0x24),  // Home
    (23, 0x25),  // Left
    (24, 0x26),  // Up
    (25, 0x27),  // Right
    (26, 0x28),  // Down
    (27, 0x29),  // Select
    (28, 0x2A),  // Print
    (29, 0x2B),  // Execute
    (30, 0x2C),  // Snapshot
    (31, 0x2D),  // Insert
    (32, 0x2E),  // Delete
    (33, 0x2F),  // Help
    (34, 0x30),  // D0
    (35, 0x31),  // D1
    (36, 0x32),  // D2
    (37, 0x33),  // D3
    (38, 0x34),  // D4
    (39, 0x35),  // D5
    (40, 0x36),  // D6
    (41, 0x37),  // D7
    (42, 0x38),  // D8
    (43, 0x39),  // D9
    (44, 0x41),  // A
    (45, 0x42),  // B
    (46, 0x43),  // C
    (47, 0x44),  // D
    (48, 0x45),  // E
    (49, 0x46),  // F
    (50, 0x47),  // G
    (51, 0x48),  // H
    (52, 0x49),  // I
    (53, 0x4A),  // J
    (54, 0x4B),  // K
    (55, 0x4C),  // L
    (56, 0x4D),  // M
    (57, 0x4E),  // N
    (58, 0x4F),  // O
    (59, 0x50),  // P
    (60, 0x51),  // Q
    (61, 0x52),  // R
    (62, 0x53),  // S
    (63, 0x54),  // T
    (64, 0x55),  // U
    (65, 0x56),  // V
    (66, 0x57),  // W
    (67, 0x58),  // X
    (68, 0x59),  // Y
    (69, 0x5A),  // Z
    (70, 0x5B),  // LWin
    (71, 0x5C),  // RWin
    (72, 0x5D),  // Apps
    (73, 0x5F),  // Sleep
    (74, 0x60),  // NumPad0
    (75, 0x61),  // NumPad1
    (76, 0x62),  // NumPad2
    (77, 0x63),  // NumPad3
    (78, 0x64),  // NumPad4
    (79, 0x65),  // NumPad5
    (80, 0x66),  // NumPad6
    (81, 0x67),  // NumPad7
    (82, 0x68),  // NumPad8
    (83, 0x69),  // NumPad9
    (84, 0x6A),  // Multiply
    (85, 0x6B),  // Add
    (86, 0x6C),  // Separator
    (87, 0x6D),  // Subtract
    (88, 0x6E),  // Decimal
    (89, 0x6F),  // Divide
    (90, 0x70),  // F1
    (91, 0x71),  // F2
    (92, 0x72),  // F3
    (93, 0x73),  // F4
    (94, 0x74),  // F5
    (95, 0x75),  // F6
    (96, 0x76),  // F7
    (97, 0x77),  // F8
    (98, 0x78),  // F9
    (99, 0x79),  // F10
    (100, 0x7A), // F11
    (101, 0x7B), // F12
    (102, 0x7C), // F13
    (103, 0x7D), // F14
    (104, 0x7E), // F15
    (105, 0x7F), // F16
    (106, 0x80), // F17
    (107, 0x81), // F18
    (108, 0x82), // F19
    (109, 0x83), // F20
    (110, 0x84), // F21
    (111, 0x85), // F22
    (112, 0x86), // F23
    (113, 0x87), // F24
    (114, 0x90), // NumLock
    (115, 0x91), // Scroll
    (116, 0xA0), // LeftShift
    (117, 0xA1), // RightShift
    (118, 0xA2), // LeftCtrl
    (119, 0xA3), // RightCtrl
    (120, 0xA4), // LeftAlt
    (121, 0xA5), // RightAlt
    (122, 0xA6), // BrowserBack
    (123, 0xA7), // BrowserForward
    (124, 0xA8), // BrowserRefresh
    (125, 0xA9), // BrowserStop
    (126, 0xAA), // BrowserSearch
    (127, 0xAB), // BrowserFavorites
    (128, 0xAC), // BrowserHome
    (129, 0xAD), // VolumeMute
    (130, 0xAE), // VolumeDown
    (131, 0xAF), // VolumeUp
    (132, 0xB0), // MediaNextTrack
    (133, 0xB1), // MediaPreviousTrack
    (134, 0xB2), // MediaStop
    (135, 0xB3), // MediaPlayPause
    (136, 0xB4), // LaunchMail
    (137, 0xB5), // SelectMedia
    (138, 0xB6), // LaunchApplication1
    (139, 0xB7), // LaunchApplication2
    (140, 0xBA), // Oem1
    (141, 0xBB), // OemPlus
    (142, 0xBC), // OemComma
    (143, 0xBD), // OemMinus
    (144, 0xBE), // OemPeriod
    (145, 0xBF), // Oem2
    (146, 0xC0), // Oem3
    (147, 0xC1), // AbntC1
    (148, 0xC2), // AbntC2
    (149, 0xDB), // Oem4
    (150, 0xDC), // Oem5
    (151, 0xDD), // Oem6
    (152, 0xDE), // Oem7
    (153, 0xDF), // Oem8
    (154, 0xE2), // Oem102
    (155, 0xE5), // ImeProcessed
    (157, 0xF0), // OemAttn
    (158, 0xF1), // OemFinish
    (159, 0xF2), // OemCopy
    (160, 0xF3), // OemAuto
    (161, 0xF4), // OemEnlw
    (162, 0xF5), // OemBackTab
    (163, 0xF6), // Attn
    (164, 0xF7), // CrSel
    (165, 0xF8), // ExSel
    (166, 0xF9), // EraseEof
    (167, 0xFA), // Play
    (168, 0xFB), // Zoom
    (169, 0xFC), // NoName
    (170, 0xFD), // Pa1
    (171, 0xFE), // OemClear
];

/// C# `Enum.GetNames<KeyId>()` as read by `Enum.TryParse(text, ignoreCase: false)`: `(exact field name, KeyId value)`,
/// declaration order, 202 rows. Aliases are separate rows with the same value (`"Return"` and `"Enter"` are both 6).
pub const KEY_NAMES: &[(&str, i32)] = &[
    ("None", 0),
    ("Cancel", 1),
    ("Back", 2),
    ("Tab", 3),
    ("LineFeed", 4),
    ("Clear", 5),
    ("Return", 6),
    ("Enter", 6),
    ("Pause", 7),
    ("Capital", 8),
    ("CapsLock", 8),
    ("KanaMode", 9),
    ("HangulMode", 9),
    ("JunjaMode", 10),
    ("FinalMode", 11),
    ("HanjaMode", 12),
    ("KanjiMode", 12),
    ("Escape", 13),
    ("ImeConvert", 14),
    ("ImeNonConvert", 15),
    ("ImeAccept", 16),
    ("ImeModeChange", 17),
    ("Space", 18),
    ("Prior", 19),
    ("PageUp", 19),
    ("Next", 20),
    ("PageDown", 20),
    ("End", 21),
    ("Home", 22),
    ("Left", 23),
    ("Up", 24),
    ("Right", 25),
    ("Down", 26),
    ("Select", 27),
    ("Print", 28),
    ("Execute", 29),
    ("Snapshot", 30),
    ("PrintScreen", 30),
    ("Insert", 31),
    ("Delete", 32),
    ("Help", 33),
    ("D0", 34),
    ("D1", 35),
    ("D2", 36),
    ("D3", 37),
    ("D4", 38),
    ("D5", 39),
    ("D6", 40),
    ("D7", 41),
    ("D8", 42),
    ("D9", 43),
    ("A", 44),
    ("B", 45),
    ("C", 46),
    ("D", 47),
    ("E", 48),
    ("F", 49),
    ("G", 50),
    ("H", 51),
    ("I", 52),
    ("J", 53),
    ("K", 54),
    ("L", 55),
    ("M", 56),
    ("N", 57),
    ("O", 58),
    ("P", 59),
    ("Q", 60),
    ("R", 61),
    ("S", 62),
    ("T", 63),
    ("U", 64),
    ("V", 65),
    ("W", 66),
    ("X", 67),
    ("Y", 68),
    ("Z", 69),
    ("LWin", 70),
    ("RWin", 71),
    ("Apps", 72),
    ("Sleep", 73),
    ("NumPad0", 74),
    ("NumPad1", 75),
    ("NumPad2", 76),
    ("NumPad3", 77),
    ("NumPad4", 78),
    ("NumPad5", 79),
    ("NumPad6", 80),
    ("NumPad7", 81),
    ("NumPad8", 82),
    ("NumPad9", 83),
    ("Multiply", 84),
    ("Add", 85),
    ("Separator", 86),
    ("Subtract", 87),
    ("Decimal", 88),
    ("Divide", 89),
    ("F1", 90),
    ("F2", 91),
    ("F3", 92),
    ("F4", 93),
    ("F5", 94),
    ("F6", 95),
    ("F7", 96),
    ("F8", 97),
    ("F9", 98),
    ("F10", 99),
    ("F11", 100),
    ("F12", 101),
    ("F13", 102),
    ("F14", 103),
    ("F15", 104),
    ("F16", 105),
    ("F17", 106),
    ("F18", 107),
    ("F19", 108),
    ("F20", 109),
    ("F21", 110),
    ("F22", 111),
    ("F23", 112),
    ("F24", 113),
    ("NumLock", 114),
    ("Scroll", 115),
    ("LeftShift", 116),
    ("RightShift", 117),
    ("LeftCtrl", 118),
    ("RightCtrl", 119),
    ("LeftAlt", 120),
    ("RightAlt", 121),
    ("BrowserBack", 122),
    ("BrowserForward", 123),
    ("BrowserRefresh", 124),
    ("BrowserStop", 125),
    ("BrowserSearch", 126),
    ("BrowserFavorites", 127),
    ("BrowserHome", 128),
    ("VolumeMute", 129),
    ("VolumeDown", 130),
    ("VolumeUp", 131),
    ("MediaNextTrack", 132),
    ("MediaPreviousTrack", 133),
    ("MediaStop", 134),
    ("MediaPlayPause", 135),
    ("LaunchMail", 136),
    ("SelectMedia", 137),
    ("LaunchApplication1", 138),
    ("LaunchApplication2", 139),
    ("Oem1", 140),
    ("OemSemicolon", 140),
    ("OemPlus", 141),
    ("OemComma", 142),
    ("OemMinus", 143),
    ("OemPeriod", 144),
    ("Oem2", 145),
    ("OemQuestion", 145),
    ("Oem3", 146),
    ("OemTilde", 146),
    ("AbntC1", 147),
    ("AbntC2", 148),
    ("Oem4", 149),
    ("OemOpenBrackets", 149),
    ("Oem5", 150),
    ("OemPipe", 150),
    ("Oem6", 151),
    ("OemCloseBrackets", 151),
    ("Oem7", 152),
    ("OemQuotes", 152),
    ("Oem8", 153),
    ("Oem102", 154),
    ("OemBackslash", 154),
    ("ImeProcessed", 155),
    ("System", 156),
    ("OemAttn", 157),
    ("DbeAlphanumeric", 157),
    ("OemFinish", 158),
    ("DbeKatakana", 158),
    ("OemCopy", 159),
    ("DbeHiragana", 159),
    ("OemAuto", 160),
    ("DbeSbcsChar", 160),
    ("OemEnlw", 161),
    ("DbeDbcsChar", 161),
    ("OemBackTab", 162),
    ("DbeRoman", 162),
    ("Attn", 163),
    ("DbeNoRoman", 163),
    ("CrSel", 164),
    ("DbeEnterWordRegisterMode", 164),
    ("ExSel", 165),
    ("DbeEnterImeConfigureMode", 165),
    ("EraseEof", 166),
    ("DbeFlushString", 166),
    ("Play", 167),
    ("DbeCodeInput", 167),
    ("Zoom", 168),
    ("DbeNoCodeInput", 168),
    ("NoName", 169),
    ("DbeDetermineString", 169),
    ("Pa1", 170),
    ("DbeEnterDialogConversionMode", 170),
    ("OemClear", 171),
    ("DeadCharProcessed", 172),
];

/// `KeyIdMapping.FromVirtualKey` (C# lines 26-36).
///
/// Rules (C# is the spec):
/// - a code outside `0..=255` (negative, 256, `i32::MIN`, `i32::MAX`) gives `KeyId::None` (value 0), it never panics;
/// - `is_extended` changes only the codes `0x11` (Control) and `0x12` (Menu): they become `RightCtrl` (119) and
///   `RightAlt` (121). Every other code ignores the flag;
/// - otherwise the row of [`VIRTUAL_KEY_TO_KEY`] for that code, or `KeyId::None` when the code has no row.
pub fn from_virtual_key(virtual_key: i32, is_extended: bool) -> KeyId {
    todo!()
}

/// `KeyIdMapping.ToVirtualKey` (C# lines 39-43): the row of [`KEY_TO_VIRTUAL_KEY`] for `key`, or `0` when the key has no code
/// (`None`, `LineFeed`, `System`, `DeadCharProcessed`) or the value is outside the enum (negative, above 172, `i32::MIN`).
pub fn to_virtual_key(key: KeyId) -> i32 {
    todo!()
}

/// `KeyIdMapping.TryParse(string name, out KeyId key)` (C# lines 49-57). `None` where C# returns `false` (C# sets `key = None`).
///
/// Order (C# is the spec):
/// 1. `pr_core::settings::shortcuts::canonicalize(Some(name))`: trim, then alias to canonical name (ASCII case-insensitive);
/// 2. reject when the result is empty, contains `','`, starts with an ASCII or Unicode digit, or starts with `'-'` or `'+'`;
/// 3. exact, case-sensitive lookup in [`KEY_NAMES`] (`Enum.TryParse(ignoreCase: false)`). Every field name is accepted, so
///    `"None"`, `"LineFeed"`, `"System"` and `"DeadCharProcessed"` are accepted too.
pub fn try_parse(name: &str) -> Option<KeyId> {
    todo!()
}
