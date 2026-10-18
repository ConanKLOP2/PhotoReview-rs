//! Port of `PhotoReview.App.Shared/Input/KeyId.cs` (C-06): a 1:1 copy of the `System.Windows.Input.Key` names and numeric
//! values, aliases included, without a WPF reference. `config.json` stores WPF `Key` names, so this vocabulary is a wire format.
//! Baseline `d92d6aae`. Generated table `KEY_ID_NAMES`: 202 declared names, 173 distinct values 0..=172, no gaps.
//!
//! Decisions (W8-05b, contract `contracts/W8-05b.md`):
//! - `KeyId` is a newtype over the raw value, not a Rust `enum`: C# aliases (`Return = Enter = 6`) share one value, and a Rust
//!   enum cannot hold two variants with one discriminant. Every declared C# name is an associated const (SCREAMING_SNAKE);
//!   aliases are equal values, so `KeyId::ENTER == KeyId::RETURN`.
//! - Parsing of user text (trimming rules are the caller's, numbers, comma lists, reserved keys) is not here:
//!   `ShortcutKeyName` (W8-04c) and `KeyIdMapping.TryParse` (W8-05c) own it. This module answers name and value lookups only.
//! - No `unsafe`, no `windows` crate. Only `pr_core::text` (OrdinalIgnoreCase) is used.
//!

/// Every declared name with its raw value, in C# declaration order (aliases included).
/// The golden `declared` cases pin this order and these values; `ContractMirrorTests` pins them against WPF `Key`.
pub const KEY_ID_NAMES: &[(&str, u8)] = &[
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

/// Canonical name per raw value 0..=172 (C# `Enum.GetName`), copied from the golden `canonical` cases: the baseline
/// runtime picks `Return` for 6, `Capital` for 8, `PageUp` for 19 and so on, not the first declared alias.
const CANONICAL_NAMES: [&str; 173] = [
    "None",                     // 0
    "Cancel",                   // 1
    "Back",                     // 2
    "Tab",                      // 3
    "LineFeed",                 // 4
    "Clear",                    // 5
    "Return",                   // 6
    "Pause",                    // 7
    "Capital",                  // 8
    "KanaMode",                 // 9
    "JunjaMode",                // 10
    "FinalMode",                // 11
    "HanjaMode",                // 12
    "Escape",                   // 13
    "ImeConvert",               // 14
    "ImeNonConvert",            // 15
    "ImeAccept",                // 16
    "ImeModeChange",            // 17
    "Space",                    // 18
    "PageUp",                   // 19
    "Next",                     // 20
    "End",                      // 21
    "Home",                     // 22
    "Left",                     // 23
    "Up",                       // 24
    "Right",                    // 25
    "Down",                     // 26
    "Select",                   // 27
    "Print",                    // 28
    "Execute",                  // 29
    "Snapshot",                 // 30
    "Insert",                   // 31
    "Delete",                   // 32
    "Help",                     // 33
    "D0",                       // 34
    "D1",                       // 35
    "D2",                       // 36
    "D3",                       // 37
    "D4",                       // 38
    "D5",                       // 39
    "D6",                       // 40
    "D7",                       // 41
    "D8",                       // 42
    "D9",                       // 43
    "A",                        // 44
    "B",                        // 45
    "C",                        // 46
    "D",                        // 47
    "E",                        // 48
    "F",                        // 49
    "G",                        // 50
    "H",                        // 51
    "I",                        // 52
    "J",                        // 53
    "K",                        // 54
    "L",                        // 55
    "M",                        // 56
    "N",                        // 57
    "O",                        // 58
    "P",                        // 59
    "Q",                        // 60
    "R",                        // 61
    "S",                        // 62
    "T",                        // 63
    "U",                        // 64
    "V",                        // 65
    "W",                        // 66
    "X",                        // 67
    "Y",                        // 68
    "Z",                        // 69
    "LWin",                     // 70
    "RWin",                     // 71
    "Apps",                     // 72
    "Sleep",                    // 73
    "NumPad0",                  // 74
    "NumPad1",                  // 75
    "NumPad2",                  // 76
    "NumPad3",                  // 77
    "NumPad4",                  // 78
    "NumPad5",                  // 79
    "NumPad6",                  // 80
    "NumPad7",                  // 81
    "NumPad8",                  // 82
    "NumPad9",                  // 83
    "Multiply",                 // 84
    "Add",                      // 85
    "Separator",                // 86
    "Subtract",                 // 87
    "Decimal",                  // 88
    "Divide",                   // 89
    "F1",                       // 90
    "F2",                       // 91
    "F3",                       // 92
    "F4",                       // 93
    "F5",                       // 94
    "F6",                       // 95
    "F7",                       // 96
    "F8",                       // 97
    "F9",                       // 98
    "F10",                      // 99
    "F11",                      // 100
    "F12",                      // 101
    "F13",                      // 102
    "F14",                      // 103
    "F15",                      // 104
    "F16",                      // 105
    "F17",                      // 106
    "F18",                      // 107
    "F19",                      // 108
    "F20",                      // 109
    "F21",                      // 110
    "F22",                      // 111
    "F23",                      // 112
    "F24",                      // 113
    "NumLock",                  // 114
    "Scroll",                   // 115
    "LeftShift",                // 116
    "RightShift",               // 117
    "LeftCtrl",                 // 118
    "RightCtrl",                // 119
    "LeftAlt",                  // 120
    "RightAlt",                 // 121
    "BrowserBack",              // 122
    "BrowserForward",           // 123
    "BrowserRefresh",           // 124
    "BrowserStop",              // 125
    "BrowserSearch",            // 126
    "BrowserFavorites",         // 127
    "BrowserHome",              // 128
    "VolumeMute",               // 129
    "VolumeDown",               // 130
    "VolumeUp",                 // 131
    "MediaNextTrack",           // 132
    "MediaPreviousTrack",       // 133
    "MediaStop",                // 134
    "MediaPlayPause",           // 135
    "LaunchMail",               // 136
    "SelectMedia",              // 137
    "LaunchApplication1",       // 138
    "LaunchApplication2",       // 139
    "Oem1",                     // 140
    "OemPlus",                  // 141
    "OemComma",                 // 142
    "OemMinus",                 // 143
    "OemPeriod",                // 144
    "OemQuestion",              // 145
    "Oem3",                     // 146
    "AbntC1",                   // 147
    "AbntC2",                   // 148
    "OemOpenBrackets",          // 149
    "Oem5",                     // 150
    "Oem6",                     // 151
    "OemQuotes",                // 152
    "Oem8",                     // 153
    "OemBackslash",             // 154
    "ImeProcessed",             // 155
    "System",                   // 156
    "OemAttn",                  // 157
    "OemFinish",                // 158
    "OemCopy",                  // 159
    "DbeSbcsChar",              // 160
    "OemEnlw",                  // 161
    "OemBackTab",               // 162
    "DbeNoRoman",               // 163
    "DbeEnterWordRegisterMode", // 164
    "DbeEnterImeConfigureMode", // 165
    "EraseEof",                 // 166
    "Play",                     // 167
    "DbeNoCodeInput",           // 168
    "NoName",                   // 169
    "Pa1",                      // 170
    "OemClear",                 // 171
    "DeadCharProcessed",        // 172
];

/// Port of C# `KeyId`: a raw WPF `Key` value (0..=172). Equality, ordering and hashing are by value, as C# enum equality is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(u8);

impl KeyId {
    // One const per declared C# name, in declaration order. Aliases repeat the value of their first declared name.
    pub const NONE: KeyId = KeyId(0);
    pub const CANCEL: KeyId = KeyId(1);
    pub const BACK: KeyId = KeyId(2);
    pub const TAB: KeyId = KeyId(3);
    pub const LINE_FEED: KeyId = KeyId(4);
    pub const CLEAR: KeyId = KeyId(5);
    pub const RETURN: KeyId = KeyId(6);
    /// Declared alias of `RETURN` (C# `Return`).
    pub const ENTER: KeyId = KeyId(6);
    pub const PAUSE: KeyId = KeyId(7);
    pub const CAPITAL: KeyId = KeyId(8);
    /// Declared alias of `CAPITAL` (C# `Capital`).
    pub const CAPS_LOCK: KeyId = KeyId(8);
    pub const KANA_MODE: KeyId = KeyId(9);
    /// Declared alias of `KANA_MODE` (C# `KanaMode`).
    pub const HANGUL_MODE: KeyId = KeyId(9);
    pub const JUNJA_MODE: KeyId = KeyId(10);
    pub const FINAL_MODE: KeyId = KeyId(11);
    pub const HANJA_MODE: KeyId = KeyId(12);
    /// Declared alias of `HANJA_MODE` (C# `HanjaMode`).
    pub const KANJI_MODE: KeyId = KeyId(12);
    pub const ESCAPE: KeyId = KeyId(13);
    pub const IME_CONVERT: KeyId = KeyId(14);
    pub const IME_NON_CONVERT: KeyId = KeyId(15);
    pub const IME_ACCEPT: KeyId = KeyId(16);
    pub const IME_MODE_CHANGE: KeyId = KeyId(17);
    pub const SPACE: KeyId = KeyId(18);
    pub const PRIOR: KeyId = KeyId(19);
    /// Declared alias of `PRIOR` (C# `Prior`).
    pub const PAGE_UP: KeyId = KeyId(19);
    pub const NEXT: KeyId = KeyId(20);
    /// Declared alias of `NEXT` (C# `Next`).
    pub const PAGE_DOWN: KeyId = KeyId(20);
    pub const END: KeyId = KeyId(21);
    pub const HOME: KeyId = KeyId(22);
    pub const LEFT: KeyId = KeyId(23);
    pub const UP: KeyId = KeyId(24);
    pub const RIGHT: KeyId = KeyId(25);
    pub const DOWN: KeyId = KeyId(26);
    pub const SELECT: KeyId = KeyId(27);
    pub const PRINT: KeyId = KeyId(28);
    pub const EXECUTE: KeyId = KeyId(29);
    pub const SNAPSHOT: KeyId = KeyId(30);
    /// Declared alias of `SNAPSHOT` (C# `Snapshot`).
    pub const PRINT_SCREEN: KeyId = KeyId(30);
    pub const INSERT: KeyId = KeyId(31);
    pub const DELETE: KeyId = KeyId(32);
    pub const HELP: KeyId = KeyId(33);
    pub const D0: KeyId = KeyId(34);
    pub const D1: KeyId = KeyId(35);
    pub const D2: KeyId = KeyId(36);
    pub const D3: KeyId = KeyId(37);
    pub const D4: KeyId = KeyId(38);
    pub const D5: KeyId = KeyId(39);
    pub const D6: KeyId = KeyId(40);
    pub const D7: KeyId = KeyId(41);
    pub const D8: KeyId = KeyId(42);
    pub const D9: KeyId = KeyId(43);
    pub const A: KeyId = KeyId(44);
    pub const B: KeyId = KeyId(45);
    pub const C: KeyId = KeyId(46);
    pub const D: KeyId = KeyId(47);
    pub const E: KeyId = KeyId(48);
    pub const F: KeyId = KeyId(49);
    pub const G: KeyId = KeyId(50);
    pub const H: KeyId = KeyId(51);
    pub const I: KeyId = KeyId(52);
    pub const J: KeyId = KeyId(53);
    pub const K: KeyId = KeyId(54);
    pub const L: KeyId = KeyId(55);
    pub const M: KeyId = KeyId(56);
    pub const N: KeyId = KeyId(57);
    pub const O: KeyId = KeyId(58);
    pub const P: KeyId = KeyId(59);
    pub const Q: KeyId = KeyId(60);
    pub const R: KeyId = KeyId(61);
    pub const S: KeyId = KeyId(62);
    pub const T: KeyId = KeyId(63);
    pub const U: KeyId = KeyId(64);
    pub const V: KeyId = KeyId(65);
    pub const W: KeyId = KeyId(66);
    pub const X: KeyId = KeyId(67);
    pub const Y: KeyId = KeyId(68);
    pub const Z: KeyId = KeyId(69);
    pub const LWIN: KeyId = KeyId(70);
    pub const RWIN: KeyId = KeyId(71);
    pub const APPS: KeyId = KeyId(72);
    pub const SLEEP: KeyId = KeyId(73);
    pub const NUM_PAD0: KeyId = KeyId(74);
    pub const NUM_PAD1: KeyId = KeyId(75);
    pub const NUM_PAD2: KeyId = KeyId(76);
    pub const NUM_PAD3: KeyId = KeyId(77);
    pub const NUM_PAD4: KeyId = KeyId(78);
    pub const NUM_PAD5: KeyId = KeyId(79);
    pub const NUM_PAD6: KeyId = KeyId(80);
    pub const NUM_PAD7: KeyId = KeyId(81);
    pub const NUM_PAD8: KeyId = KeyId(82);
    pub const NUM_PAD9: KeyId = KeyId(83);
    pub const MULTIPLY: KeyId = KeyId(84);
    pub const ADD: KeyId = KeyId(85);
    pub const SEPARATOR: KeyId = KeyId(86);
    pub const SUBTRACT: KeyId = KeyId(87);
    pub const DECIMAL: KeyId = KeyId(88);
    pub const DIVIDE: KeyId = KeyId(89);
    pub const F1: KeyId = KeyId(90);
    pub const F2: KeyId = KeyId(91);
    pub const F3: KeyId = KeyId(92);
    pub const F4: KeyId = KeyId(93);
    pub const F5: KeyId = KeyId(94);
    pub const F6: KeyId = KeyId(95);
    pub const F7: KeyId = KeyId(96);
    pub const F8: KeyId = KeyId(97);
    pub const F9: KeyId = KeyId(98);
    pub const F10: KeyId = KeyId(99);
    pub const F11: KeyId = KeyId(100);
    pub const F12: KeyId = KeyId(101);
    pub const F13: KeyId = KeyId(102);
    pub const F14: KeyId = KeyId(103);
    pub const F15: KeyId = KeyId(104);
    pub const F16: KeyId = KeyId(105);
    pub const F17: KeyId = KeyId(106);
    pub const F18: KeyId = KeyId(107);
    pub const F19: KeyId = KeyId(108);
    pub const F20: KeyId = KeyId(109);
    pub const F21: KeyId = KeyId(110);
    pub const F22: KeyId = KeyId(111);
    pub const F23: KeyId = KeyId(112);
    pub const F24: KeyId = KeyId(113);
    pub const NUM_LOCK: KeyId = KeyId(114);
    pub const SCROLL: KeyId = KeyId(115);
    pub const LEFT_SHIFT: KeyId = KeyId(116);
    pub const RIGHT_SHIFT: KeyId = KeyId(117);
    pub const LEFT_CTRL: KeyId = KeyId(118);
    pub const RIGHT_CTRL: KeyId = KeyId(119);
    pub const LEFT_ALT: KeyId = KeyId(120);
    pub const RIGHT_ALT: KeyId = KeyId(121);
    pub const BROWSER_BACK: KeyId = KeyId(122);
    pub const BROWSER_FORWARD: KeyId = KeyId(123);
    pub const BROWSER_REFRESH: KeyId = KeyId(124);
    pub const BROWSER_STOP: KeyId = KeyId(125);
    pub const BROWSER_SEARCH: KeyId = KeyId(126);
    pub const BROWSER_FAVORITES: KeyId = KeyId(127);
    pub const BROWSER_HOME: KeyId = KeyId(128);
    pub const VOLUME_MUTE: KeyId = KeyId(129);
    pub const VOLUME_DOWN: KeyId = KeyId(130);
    pub const VOLUME_UP: KeyId = KeyId(131);
    pub const MEDIA_NEXT_TRACK: KeyId = KeyId(132);
    pub const MEDIA_PREVIOUS_TRACK: KeyId = KeyId(133);
    pub const MEDIA_STOP: KeyId = KeyId(134);
    pub const MEDIA_PLAY_PAUSE: KeyId = KeyId(135);
    pub const LAUNCH_MAIL: KeyId = KeyId(136);
    pub const SELECT_MEDIA: KeyId = KeyId(137);
    pub const LAUNCH_APPLICATION1: KeyId = KeyId(138);
    pub const LAUNCH_APPLICATION2: KeyId = KeyId(139);
    pub const OEM1: KeyId = KeyId(140);
    /// Declared alias of `OEM1` (C# `Oem1`).
    pub const OEM_SEMICOLON: KeyId = KeyId(140);
    pub const OEM_PLUS: KeyId = KeyId(141);
    pub const OEM_COMMA: KeyId = KeyId(142);
    pub const OEM_MINUS: KeyId = KeyId(143);
    pub const OEM_PERIOD: KeyId = KeyId(144);
    pub const OEM2: KeyId = KeyId(145);
    /// Declared alias of `OEM2` (C# `Oem2`).
    pub const OEM_QUESTION: KeyId = KeyId(145);
    pub const OEM3: KeyId = KeyId(146);
    /// Declared alias of `OEM3` (C# `Oem3`).
    pub const OEM_TILDE: KeyId = KeyId(146);
    pub const ABNT_C1: KeyId = KeyId(147);
    pub const ABNT_C2: KeyId = KeyId(148);
    pub const OEM4: KeyId = KeyId(149);
    /// Declared alias of `OEM4` (C# `Oem4`).
    pub const OEM_OPEN_BRACKETS: KeyId = KeyId(149);
    pub const OEM5: KeyId = KeyId(150);
    /// Declared alias of `OEM5` (C# `Oem5`).
    pub const OEM_PIPE: KeyId = KeyId(150);
    pub const OEM6: KeyId = KeyId(151);
    /// Declared alias of `OEM6` (C# `Oem6`).
    pub const OEM_CLOSE_BRACKETS: KeyId = KeyId(151);
    pub const OEM7: KeyId = KeyId(152);
    /// Declared alias of `OEM7` (C# `Oem7`).
    pub const OEM_QUOTES: KeyId = KeyId(152);
    pub const OEM8: KeyId = KeyId(153);
    pub const OEM102: KeyId = KeyId(154);
    /// Declared alias of `OEM102` (C# `Oem102`).
    pub const OEM_BACKSLASH: KeyId = KeyId(154);
    pub const IME_PROCESSED: KeyId = KeyId(155);
    pub const SYSTEM: KeyId = KeyId(156);
    pub const OEM_ATTN: KeyId = KeyId(157);
    /// Declared alias of `OEM_ATTN` (C# `OemAttn`).
    pub const DBE_ALPHANUMERIC: KeyId = KeyId(157);
    pub const OEM_FINISH: KeyId = KeyId(158);
    /// Declared alias of `OEM_FINISH` (C# `OemFinish`).
    pub const DBE_KATAKANA: KeyId = KeyId(158);
    pub const OEM_COPY: KeyId = KeyId(159);
    /// Declared alias of `OEM_COPY` (C# `OemCopy`).
    pub const DBE_HIRAGANA: KeyId = KeyId(159);
    pub const OEM_AUTO: KeyId = KeyId(160);
    /// Declared alias of `OEM_AUTO` (C# `OemAuto`).
    pub const DBE_SBCS_CHAR: KeyId = KeyId(160);
    pub const OEM_ENLW: KeyId = KeyId(161);
    /// Declared alias of `OEM_ENLW` (C# `OemEnlw`).
    pub const DBE_DBCS_CHAR: KeyId = KeyId(161);
    pub const OEM_BACK_TAB: KeyId = KeyId(162);
    /// Declared alias of `OEM_BACK_TAB` (C# `OemBackTab`).
    pub const DBE_ROMAN: KeyId = KeyId(162);
    pub const ATTN: KeyId = KeyId(163);
    /// Declared alias of `ATTN` (C# `Attn`).
    pub const DBE_NO_ROMAN: KeyId = KeyId(163);
    pub const CR_SEL: KeyId = KeyId(164);
    /// Declared alias of `CR_SEL` (C# `CrSel`).
    pub const DBE_ENTER_WORD_REGISTER_MODE: KeyId = KeyId(164);
    pub const EX_SEL: KeyId = KeyId(165);
    /// Declared alias of `EX_SEL` (C# `ExSel`).
    pub const DBE_ENTER_IME_CONFIGURE_MODE: KeyId = KeyId(165);
    pub const ERASE_EOF: KeyId = KeyId(166);
    /// Declared alias of `ERASE_EOF` (C# `EraseEof`).
    pub const DBE_FLUSH_STRING: KeyId = KeyId(166);
    pub const PLAY: KeyId = KeyId(167);
    /// Declared alias of `PLAY` (C# `Play`).
    pub const DBE_CODE_INPUT: KeyId = KeyId(167);
    pub const ZOOM: KeyId = KeyId(168);
    /// Declared alias of `ZOOM` (C# `Zoom`).
    pub const DBE_NO_CODE_INPUT: KeyId = KeyId(168);
    pub const NO_NAME: KeyId = KeyId(169);
    /// Declared alias of `NO_NAME` (C# `NoName`).
    pub const DBE_DETERMINE_STRING: KeyId = KeyId(169);
    pub const PA1: KeyId = KeyId(170);
    /// Declared alias of `PA1` (C# `Pa1`).
    pub const DBE_ENTER_DIALOG_CONVERSION_MODE: KeyId = KeyId(170);
    pub const OEM_CLEAR: KeyId = KeyId(171);
    pub const DEAD_CHAR_PROCESSED: KeyId = KeyId(172);

    /// The raw WPF value, C# `(int)key`. Always 0..=172 for a value built by this module.
    pub const fn value(self) -> i32 {
        self.0 as i32
    }

    /// C# `Enum.IsDefined((KeyId)value)`: `Some` exactly for 0..=172, `None` for every other `i32` (negative, 173 and up).
    pub fn from_value(value: i32) -> Option<KeyId> {
        let raw = u8::try_from(value).ok()?;
        (usize::from(raw) < CANONICAL_NAMES.len()).then_some(KeyId(raw))
    }

    /// The canonical name of this value (C# `Enum.GetName`). Exactly one name per value. Which alias the baseline runtime
    /// returns is pinned by the golden `canonical` cases; the porter fills the table from those answers, not from declaration order.
    pub fn name(self) -> &'static str {
        CANONICAL_NAMES.get(usize::from(self.0)).copied().unwrap_or("None")
    }

    /// Name lookup as C# `Enum.TryParse(text, ignoreCase: false)` followed by `Enum.IsDefined`, for names only.
    /// Surrounding whitespace is ignored (golden `from_name`, probes with spaces and tabs). Numbers are NOT accepted here:
    /// the callers (`ShortcutKeyName`, `KeyIdMapping.TryParse`) reject or handle them.
    pub fn from_name(name: &str) -> Option<KeyId> {
        from_names(name, |candidate, token| candidate == token)
    }

    /// Same as [`KeyId::from_name`] with `ignoreCase: true`. Compares with `pr_core::text::eq_ignore_case`
    /// (OrdinalIgnoreCase: no `ı`/`ſ`/`K` (Kelvin) folding), never `to_lowercase`.
    pub fn from_name_ignore_case(name: &str) -> Option<KeyId> {
        from_names(name, pr_core::text::eq_ignore_case)
    }
}

/// `Enum.TryParse` over comma-separated names: each token is trimmed and looked up in `KEY_ID_NAMES` (first match), and the
/// values are OR-combined (golden: `"Tab,Back"` is `Some(TAB)`, 3 = 3 | 2). The result must be `Enum.IsDefined`. An empty token
/// (`"Tab,"`, `""`, whitespace only) or an unknown name gives `None`.
fn from_names(text: &str, same: fn(&str, &str) -> bool) -> Option<KeyId> {
    let mut bits: i32 = 0;
    for token in text.split(',') {
        let token = token.trim();
        if token.is_empty() {
            return None;
        }
        let row = KEY_ID_NAMES.iter().find(|row| same(row.0, token))?;
        bits |= i32::from(row.1);
    }
    KeyId::from_value(bits)
}
