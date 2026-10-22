//! Port of `PhotoReview.App.Shared/Input/ReviewCommand.cs`: the command kinds and the routed command record.
//!
//! The `ReviewCommandType` values are the C# enum's implicit values (0..=29, declaration order). Do not reorder.
//! Baseline `d92d6aae`. The shortcut routing that produces these commands is `input::shortcut_router` (W8-05d).

/// C# `ReviewCommandType`. Variant order = C# declaration order = implicit values 0..=29.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReviewCommandType {
    Fullscreen = 0,
    ExitFullscreen = 1,
    Close = 2,
    NextFolder = 3,
    PreviousFolder = 4,
    FirstImage = 5,
    LastImage = 6,
    Undo = 7,
    ToggleCompare = 8,
    RunAction = 9,
    Recycle = 10,
    Skip = 11,
    ToggleFit = 12,
    ZoomIn = 13,
    ZoomOut = 14,
    ZoomActualSize = 15,
    ToggleInfoOverlay = 16,
    Next = 17,
    Previous = 18,
    MoveToFolder = 19,
    CopyToFolder = 20,
    ClickZoom = 21,
    FitWidth = 22,
    FitHeight = 23,
    ToggleKeepZoom = 24,
    OpenFolder = 25,
    CustomZoom = 26,
    ToggleCaptureMember = 27,
    FitWidth2 = 28,
    Refresh = 29,
    /// W9-E7 (FEAT-EXTERNAL-EDITORS): sends the current photo to the default external editor. C# declares it right after `Refresh` (value 30);
    /// FEAT-RENAME's `RenameFile` follows it (value 31, not part of lane W9-E).
    OpenInDefaultEditor = 30,
}

impl ReviewCommandType {
    /// C# `IgnoresAutoRepeat(this ReviewCommandType)` (`ReviewCommandTypeExtensions`). True for exactly these 14 kinds:
    /// Recycle, RunAction, Undo, MoveToFolder, CopyToFolder, ToggleInfoOverlay, Fullscreen, ToggleCompare, ClickZoom,
    /// ToggleKeepZoom, OpenFolder, CustomZoom, ToggleCaptureMember, Refresh. Every other kind is false.
    pub fn ignores_auto_repeat(self) -> bool {
        matches!(
            self,
            Self::Recycle
                | Self::RunAction
                | Self::Undo
                | Self::MoveToFolder
                | Self::CopyToFolder
                | Self::ToggleInfoOverlay
                | Self::Fullscreen
                | Self::ToggleCompare
                | Self::ClickZoom
                | Self::ToggleKeepZoom
                | Self::OpenFolder
                | Self::CustomZoom
                | Self::ToggleCaptureMember
                | Self::Refresh
        )
    }
}

/// C# `ReviewCommand` (`readonly record struct`, positional `(ReviewCommandType Type, int ActionIndex = -1, bool ForcePicker = false)`).
///
/// Construct with [`ReviewCommand::new`] or one of the constants. There is deliberately no `Default`: C# `default(ReviewCommand)`
/// has `ActionIndex == 0`, while the ordinary constructor has `-1`, and the two must not be mixed up silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReviewCommand {
    pub kind: ReviewCommandType,
    /// `ActionIndex`: `-1` unless the command is `RunAction`.
    pub action_index: i32,
    /// `ForcePicker`: "Move to / Copy to" pressed with Shift (always open the folder picker).
    pub force_picker: bool,
}

impl ReviewCommand {
    /// C# `new ReviewCommand(type)`: `action_index = -1`, `force_picker = false`.
    pub const fn new(kind: ReviewCommandType) -> Self {
        Self {
            kind,
            action_index: -1,
            force_picker: false,
        }
    }

    /// C# `ReviewCommand.Action(int index)`: `RunAction` with `action_index = index`.
    pub fn action(index: i32) -> Self {
        Self {
            kind: ReviewCommandType::RunAction,
            action_index: index,
            force_picker: false,
        }
    }

    /// C# `ReviewCommand.MoveToFolder(bool forcePicker)`.
    pub fn move_to_folder(force_picker: bool) -> Self {
        Self {
            force_picker,
            ..Self::new(ReviewCommandType::MoveToFolder)
        }
    }

    /// C# `ReviewCommand.CopyToFolder(bool forcePicker)`.
    pub fn copy_to_folder(force_picker: bool) -> Self {
        Self {
            force_picker,
            ..Self::new(ReviewCommandType::CopyToFolder)
        }
    }

    // The 27 C# static properties (`ReviewCommand.Fullscreen`, ...): each is `new(kind)`. Associated constants, not functions.
    pub const FULLSCREEN: Self = Self::new(ReviewCommandType::Fullscreen);
    pub const EXIT_FULLSCREEN: Self = Self::new(ReviewCommandType::ExitFullscreen);
    pub const CLOSE: Self = Self::new(ReviewCommandType::Close);
    pub const NEXT_FOLDER: Self = Self::new(ReviewCommandType::NextFolder);
    pub const PREVIOUS_FOLDER: Self = Self::new(ReviewCommandType::PreviousFolder);
    pub const FIRST_IMAGE: Self = Self::new(ReviewCommandType::FirstImage);
    pub const LAST_IMAGE: Self = Self::new(ReviewCommandType::LastImage);
    pub const UNDO: Self = Self::new(ReviewCommandType::Undo);
    pub const TOGGLE_COMPARE: Self = Self::new(ReviewCommandType::ToggleCompare);
    pub const RECYCLE: Self = Self::new(ReviewCommandType::Recycle);
    pub const SKIP: Self = Self::new(ReviewCommandType::Skip);
    pub const TOGGLE_FIT: Self = Self::new(ReviewCommandType::ToggleFit);
    pub const ZOOM_IN: Self = Self::new(ReviewCommandType::ZoomIn);
    pub const ZOOM_OUT: Self = Self::new(ReviewCommandType::ZoomOut);
    pub const ZOOM_ACTUAL_SIZE: Self = Self::new(ReviewCommandType::ZoomActualSize);
    pub const TOGGLE_INFO_OVERLAY: Self = Self::new(ReviewCommandType::ToggleInfoOverlay);
    pub const NEXT: Self = Self::new(ReviewCommandType::Next);
    pub const PREVIOUS: Self = Self::new(ReviewCommandType::Previous);
    pub const CLICK_ZOOM: Self = Self::new(ReviewCommandType::ClickZoom);
    pub const FIT_WIDTH: Self = Self::new(ReviewCommandType::FitWidth);
    pub const FIT_HEIGHT: Self = Self::new(ReviewCommandType::FitHeight);
    pub const TOGGLE_KEEP_ZOOM: Self = Self::new(ReviewCommandType::ToggleKeepZoom);
    pub const OPEN_FOLDER: Self = Self::new(ReviewCommandType::OpenFolder);
    pub const CUSTOM_ZOOM: Self = Self::new(ReviewCommandType::CustomZoom);
    pub const TOGGLE_CAPTURE_MEMBER: Self = Self::new(ReviewCommandType::ToggleCaptureMember);
    pub const FIT_WIDTH2: Self = Self::new(ReviewCommandType::FitWidth2);
    pub const REFRESH: Self = Self::new(ReviewCommandType::Refresh);
    /// W9-E7: C# `ReviewCommand.OpenInDefaultEditor`.
    pub const OPEN_IN_DEFAULT_EDITOR: Self = Self::new(ReviewCommandType::OpenInDefaultEditor);
}
