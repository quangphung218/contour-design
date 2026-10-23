//! UI strings in English and Vietnamese (spec 1.5 i18n: en and vi first).
//!
//! Every visible string goes through [`t`] or [`tf`]. The table is one
//! macro call, so a key without both translations does not compile.
//! Vietnamese follows the vi-VN UI conventions: impersonal, kiểu mới tone
//! marks, product terms (agent, preview, deck, template…) kept in English.

use std::cell::Cell;

/// A UI language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Locale {
    #[default]
    En,
    Vi,
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::En, Self::Vi];

    /// BCP 47 tag, also the value stored as `locale` in app settings.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Vi => "vi",
        }
    }

    /// The language's own name, for the picker.
    #[must_use]
    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Vi => "Tiếng Việt",
        }
    }

    /// `vi`, `vi-VN`… map to Vietnamese; anything else to English.
    #[must_use]
    pub fn parse(tag: &str) -> Self {
        let primary = tag.split(['-', '_']).next().unwrap_or("");
        if primary.eq_ignore_ascii_case("vi") {
            Self::Vi
        } else {
            Self::En
        }
    }
}

thread_local! {
    static CURRENT: Cell<Locale> = const { Cell::new(Locale::En) };
}

/// Sets the language [`t`] uses. The app re-renders after a change.
pub fn set_locale(locale: Locale) {
    CURRENT.with(|c| c.set(locale));
}

#[must_use]
pub fn locale() -> Locale {
    CURRENT.with(Cell::get)
}

macro_rules! messages {
    ($($key:ident => $en:literal, $vi:literal;)*) => {
        /// A UI string key.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum T { $($key),* }

        impl T {
            pub const ALL: &'static [T] = &[$(T::$key),*];

            #[must_use]
            pub fn text(self, locale: Locale) -> &'static str {
                match locale {
                    Locale::En => match self { $(T::$key => $en),* },
                    Locale::Vi => match self { $(T::$key => $vi),* },
                }
            }
        }
    };
}

messages! {
    // Shell
    NavHome => "Home", "Trang chủ";
    NavProjects => "Projects", "Project";
    NavSettings => "Settings", "Cài đặt";
    PageNotFound => "Page not found.", "Không tìm thấy trang.";
    JustNow => "just now", "vừa xong";
    MinAgo => "{0} min ago", "{0} phút trước";
    HoursAgo => "{0} h ago", "{0} giờ trước";
    DaysAgo => "{0} d ago", "{0} ngày trước";
    Loading => "Loading…", "Đang tải…";
    Cancel => "Cancel", "Huỷ";
    Save => "Save", "Lưu";
    Saved => "Saved", "Đã lưu";
    Delete => "Delete", "Xoá";
    Close => "Close", "Đóng";
    Refresh => "Refresh", "Làm mới";
    DaemonUnreachable => "Cannot reach the daemon: {0}", "Không kết nối được daemon: {0}";
    UnexpectedResponse => "Unexpected response: {0}", "Phản hồi không hợp lệ: {0}";

    // Home
    HomeTitle => "What do you want to design?", "Hôm nay thiết kế gì?";
    BriefPlaceholder => "Describe a page, an app screen or a deck…", "Mô tả một trang web, một màn hình app hay một deck…";
    OutputType => "Output type", "Loại đầu ra";
    ChipPrototype => "Prototype", "Prototype";
    ChipDeck => "Slide deck", "Slide deck";
    Create => "Create", "Tạo";
    Creating => "Creating…", "Đang tạo…";
    DesignSystem => "Design system", "Design system";
    NoneOption => "None", "Không dùng";
    NewDesignSystem => "New design system…", "Tạo design system…";
    DesignSystemNamePlaceholder => "Name, e.g. Tea House", "Tên, ví dụ Tea House";
    DesignSystemBriefPlaceholder => "Describe the brand: mood, colours, type, audience…", "Mô tả thương hiệu: không khí, màu, kiểu chữ, đối tượng…";
    GenerateDesignSystem => "Generate", "Tạo";
    GeneratingDesignSystem => "Generating…", "Đang tạo…";
    GeneratingDesignSystemHint => "The agent is writing DESIGN.md and tokens.css…", "Agent đang viết DESIGN.md và tokens.css…";
    DesignSystemCreated => "Created design system {0}.", "Đã tạo design system {0}.";
    DesignSystemFailed => "Could not create the design system: {0}", "Không tạo được design system: {0}";
    Template => "Template", "Template";
    DefaultTemplate => "Default for this type", "Mặc định theo loại";
    RecentProjects => "Recent projects", "Project gần đây";
    AllProjects => "All projects", "Tất cả project";
    NoProjectsFirstBrief => "No projects yet. Your first brief creates one.", "Chưa có project nào. Brief đầu tiên sẽ tạo project.";
    KindPrototype => "Prototype", "Prototype";
    KindDeck => "Deck", "Deck";
    KindProject => "Project", "Project";

    // Projects
    ProjectsTitle => "Projects", "Project";
    OpenFolder => "Open folder…", "Mở thư mục…";
    NewProject => "New project", "Project mới";
    NoProjects => "No projects yet.", "Chưa có project nào.";
    Rename => "Rename", "Đổi tên";
    Duplicate => "Duplicate", "Nhân bản";
    ConfirmDeleteProject => "Delete this project?", "Xoá project này?";
    FolderPathLabel => "Folder path", "Đường dẫn thư mục";
    FolderPathPlaceholder => "/Users/me/my-site", "/Users/me/my-site";
    OpenAsProject => "Open as project", "Mở thành project";
    FolderHint => "The project points at this folder; files stay where they are.", "Project trỏ thẳng vào thư mục này, file không bị sao chép.";

    // Onboarding
    OnboardingTitle => "Choose your coding agent", "Chọn coding agent";
    OnboardingIntro => "Open Design drives a coding agent installed on this machine. Nothing leaves your computer except what the agent itself sends to its provider.", "Open Design điều khiển một coding agent cài sẵn trên máy. Không có dữ liệu nào rời khỏi máy, trừ những gì agent tự gửi tới provider của nó.";
    DetectingAgents => "Detecting agents…", "Đang tìm agent…";
    AgentDetectionUnavailable => "Agent detection unavailable: {0}", "Không dò được agent: {0}";
    NoAgentsFound => "No agents found. Install Claude Code or Codex, then refresh.", "Không tìm thấy agent nào. Cài Claude Code hoặc Codex rồi làm mới.";
    SkipForNow => "Skip for now", "Để sau";
    Continue => "Continue", "Tiếp tục";
    Installed => "Installed", "Đã cài";
    InstalledVersion => "Installed · {0}", "Đã cài · {0}";
    NotInstalled => "Not installed", "Chưa cài";

    // Settings
    SettingsTitle => "Settings", "Cài đặt";
    Agent => "Agent", "Agent";
    AgentId => "Agent id", "Agent id";
    Model => "Model", "Model";
    ModelPlaceholder => "Agent default", "Mặc định của agent";
    ModelHint => "Leave empty to use the agent's own default model.", "Để trống thì agent dùng model mặc định của nó.";
    Permissions => "Permissions", "Quyền";
    PermAuto => "Automatic", "Tự động";
    PermAutoDetail => "The agent edits files and runs commands in the project folder without asking.", "Agent tự sửa file và chạy lệnh trong thư mục project, không hỏi lại.";
    PermAsk => "Ask first", "Hỏi trước";
    PermAskDetail => "Claude only accepts file edits; other agents run without their auto-approve flag.", "Claude chỉ được sửa file; agent khác chạy không kèm cờ tự duyệt.";
    Appearance => "Appearance", "Giao diện";
    ThemeSystem => "System", "Theo hệ thống";
    ThemeLight => "Light", "Sáng";
    ThemeDark => "Dark", "Tối";
    Language => "Language", "Ngôn ngữ";

    // Studio: files and tabs
    ProjectFiles => "Project files", "File của project";
    RefreshFiles => "Refresh files", "Làm mới danh sách file";
    NoFilesYet => "No files yet", "Chưa có file";
    CloseTab => "Close tab", "Đóng tab";
    QuickOpen => "Go to file (Ctrl/Cmd+P)", "Mở file (Ctrl/Cmd+P)";
    SwitcherPlaceholder => "Type a file name…", "Gõ tên file…";
    SwitcherEmpty => "No matching files", "Không có file khớp";
    RecentBadge => "recent", "gần đây";

    // Studio: preview
    Preview => "Preview", "Preview";
    ReloadPreview => "Reload preview", "Tải lại preview";
    PreviewEmpty => "The preview appears when the agent writes an HTML file.", "Preview sẽ hiện khi agent ghi một file HTML.";
    DeviceDesktop => "Desktop", "Desktop";
    DeviceTablet => "Tablet", "Tablet";
    DevicePhone => "Phone", "Điện thoại";
    FirstSlide => "First slide", "Slide đầu";
    PrevSlide => "Previous slide", "Slide trước";
    NextSlide => "Next slide", "Slide sau";
    LastSlide => "Last slide", "Slide cuối";
    SlideOf => "{0} / {1}", "{0} / {1}";
    Present => "Present", "Trình chiếu";
    ExitPresent => "Exit (Esc)", "Thoát (Esc)";
    SpeakerNotes => "Notes", "Ghi chú";
    NoSpeakerNotes => "No speaker notes for this slide.", "Slide này chưa có ghi chú cho người trình bày.";
    Slides => "Slides", "Danh sách slide";
    SlideN => "Slide {0}", "Slide {0}";

    // Studio: comments
    CommentMode => "Comment", "Bình luận";
    CommentModeOn => "Click an element to comment", "Bấm vào một phần tử để bình luận";
    CommentPlaceholder => "What should change here?", "Cần sửa gì ở đây?";
    AddComment => "Add comment", "Thêm bình luận";
    RemoveComment => "Remove", "Gỡ";
    Detach => "Don't send", "Không gửi";
    AttachToNext => "Send with next message", "Gửi kèm tin nhắn sau";
    CommentsAttached => "Comments sent with the next message", "Bình luận gửi kèm tin nhắn sau";
    ElementFallback => "element", "phần tử";
    CommentFailed => "Could not save the comment: {0}", "Không lưu được bình luận: {0}";

    // Studio: preview tools
    ToolInspect => "Inspect", "Inspect";
    ToolEditText => "Edit text", "Sửa chữ";
    ToolDraw => "Draw", "Vẽ";
    ToolTweaks => "Tweaks", "Tweaks";
    InspectHint => "Click an element to inspect its style", "Bấm vào một phần tử để xem style";
    EditTextHint => "Click a text to edit it. Enter saves, Esc cancels.", "Bấm vào chữ để sửa. Enter để lưu, Esc để huỷ.";
    DrawHint => "Draw on the preview, then attach it to the next message", "Vẽ lên preview rồi gửi kèm tin nhắn sau";
    InspectEmpty => "Pick an element in the preview.", "Chọn một phần tử trong preview.";
    InspectMissing => "This element is no longer in the page.", "Phần tử này không còn trong trang.";
    ResetProp => "Reset", "Đặt lại";
    ResetAll => "Reset all", "Đặt lại hết";
    ExtractCss => "Extract CSS", "Trích CSS";
    SaveToSource => "Save to source", "Ghi vào source";
    NoOverrides => "No unsaved style changes.", "Chưa có thay đổi style nào.";
    UnsavedOverrides => "{0} unsaved style changes", "{0} thay đổi style chưa ghi";
    SavedVersion => "Saved to {0} as v{1}", "Đã ghi vào {0}, thành v{1}";
    SaveFailed => "Could not write to the source: {0}", "Không ghi được vào source: {0}";
    PaletteLabel => "Palette", "Bảng màu";
    PaletteOriginal => "Original colors", "Màu gốc";
    PaletteCoral => "Coral", "Coral";
    PaletteElectric => "Electric", "Electric";
    PaletteAcidForest => "Acid forest", "Acid forest";
    PaletteRisograph => "Risograph", "Risograph";
    PaletteMonoNoir => "Mono noir", "Mono noir";
    TweaksEmpty => "This artifact declares no tweaks.", "Artifact này không khai báo tweak nào.";
    TweaksReset => "Reset tweaks", "Đặt lại tweak";
    LintBadge => "Lint · {0}", "Lint · {0}";
    LintClean => "Lint ✓", "Lint ✓";
    LintTitle => "Lint of {0}", "Lint của {0}";
    LintNone => "No anti-slop issues found.", "Không thấy lỗi AI slop nào.";
    LintFix => "Fix: {0}", "Cách sửa: {0}";
    LintSend => "Ask the agent to fix", "Nhờ agent sửa";
    LintFailed => "Lint failed: {0}", "Không lint được: {0}";
    DrawPen => "Pen", "Bút";
    DrawBox => "Box", "Khung";
    DrawClear => "Clear", "Xoá nét";
    DrawAttach => "Attach to next message", "Gửi kèm tin nhắn sau";
    DrawAttached => "Annotation attached to the next message", "Đã đính kèm chú thích vào tin nhắn sau";
    DrawAttachedNoSnapshot => "Annotation attached (without a page snapshot)", "Đã đính kèm chú thích (không kèm ảnh chụp trang)";
    DrawFailed => "Could not attach the annotation: {0}", "Không đính kèm được chú thích: {0}";
    FilesAttached => "Files sent with the next message", "File gửi kèm tin nhắn sau";
    DeliverableNoArtifact => "The run ended without writing any file.", "Run đã xong nhưng không ghi file nào.";
    DeliverableNotTouched => "The run did not update the entry file {0}.", "Run không cập nhật file chính {0}.";
    DeliverableEntryMissing => "No entry file found for this project.", "Không tìm thấy file chính của project.";
    DeliverableTypeMismatch => "The entry file {0} is not the kind this project delivers.", "File chính {0} không đúng loại mà project cần.";
    DeliverableUnreadable => "The entry file {0} cannot be read.", "Không đọc được file chính {0}.";
    DeliverableProjectMissing => "The project folder is missing.", "Không thấy thư mục project.";

    // Studio: versions
    History => "History", "Lịch sử";
    VersionsTitle => "Versions of {0}", "Các phiên bản của {0}";
    VersionsOnlyHtml => "History is kept for HTML files only.", "Chỉ file HTML mới có lịch sử phiên bản.";
    NoVersions => "No versions yet.", "Chưa có phiên bản nào.";
    VersionLabel => "v{0}", "v{0}";
    SourceAi => "AI", "AI";
    SourceManual => "Manual", "Sửa tay";
    SourceRestore => "Restore", "Khôi phục";
    SourceSync => "Sync", "Đồng bộ";
    Current => "current", "hiện tại";
    View => "View", "Xem";
    Restore => "Restore", "Khôi phục";
    BackToLive => "Back to live preview", "Về preview hiện tại";
    ViewingVersion => "Viewing v{0}", "Đang xem v{0}";
    RestoreFailed => "Restore failed: {0}", "Không khôi phục được: {0}";

    // Studio: export and handoff
    Export => "Export", "Xuất";
    ExportHtml => "HTML (single file)", "HTML (một file)";
    ExportZip => "ZIP (whole project)", "ZIP (cả project)";
    ExportPdf => "PDF", "PDF";
    ExportPng => "PNG image", "Ảnh PNG";
    ExportPptx => "PowerPoint (PPTX)", "PowerPoint (PPTX)";
    Exporting => "Exporting {0}…", "Đang xuất {0}…";
    Exported => "Exported {0}", "Đã xuất {0}";
    ExportFailed => "Export failed: {0}", "Xuất không thành công: {0}";
    Handoff => "Handoff", "Bàn giao";
    HandoffOpenFolder => "Open project folder", "Mở thư mục project";
    HandoffCopyPrompt => "Copy prompt for a coding agent", "Chép prompt cho coding agent";
    PromptCopied => "Prompt copied", "Đã chép prompt";
    FolderOpened => "Folder opened", "Đã mở thư mục";
    HandoffFailed => "Handoff failed: {0}", "Bàn giao không thành công: {0}";

    // Studio: chat
    Chat => "Chat", "Chat";
    Conversation => "Conversation", "Cuộc trò chuyện";
    ConversationN => "Conversation {0}", "Cuộc trò chuyện {0}";
    NewConversation => "New conversation", "Cuộc trò chuyện mới";
    Fork => "Fork from here", "Rẽ nhánh từ đây";
    ForkTitle => "Fork of {0}", "Nhánh của {0}";
    DescribeChange => "Describe what to build or change.", "Mô tả thứ cần làm hoặc cần sửa.";
    ComposerPlaceholder => "Ask for a change…  (Enter to send, Shift+Enter for a new line)", "Nhập yêu cầu sửa…  (Enter để gửi, Shift+Enter để xuống dòng)";
    Send => "Send", "Gửi";
    Queue => "Queue", "Xếp hàng";
    Stop => "Stop", "Dừng";
    Stopped => "Stopped", "Đã dừng";
    Working => "Working", "Đang làm";
    Thinking => "Thinking", "Suy nghĩ";
    Answers => "Answers", "Câu trả lời";
    PreparingQuestions => "Preparing questions…", "Đang chuẩn bị câu hỏi…";
    PleaseAnswer => "Please answer: {0}", "Cần trả lời: {0}";
    SendAnswers => "Send answers", "Gửi câu trả lời";
    Sent => "Sent", "Đã gửi";
    Choose => "Choose…", "Chọn…";
    NoAgentSelected => "No coding agent selected. Choose one in Settings.", "Chưa chọn coding agent. Vào Cài đặt để chọn.";
    LostRunStream => "Lost the connection to the run stream.", "Mất kết nối tới luồng của run.";
    CannotOpenRunStream => "Could not open the run stream.", "Không mở được luồng của run.";
    StopFailed => "Stop failed: {0}", "Không dừng được: {0}";
    ToolLoopStopped => "Stopped: {0} failed {1} times in a row", "Đã dừng: {0} lỗi {1} lần liên tiếp";
    Queued => "Queued", "Đang chờ";
    QueuedHint => "Sent when the current turn ends", "Sẽ gửi khi lượt hiện tại xong";
    Steer => "Steer", "Chen vào";
    SteerHint => "Send into the running turn now", "Gửi ngay vào lượt đang chạy";
    Steered => "Steered: {0}", "Đã chen vào: {0}";
    SteerFailed => "Steer failed: {0}", "Không chen vào được: {0}";
    MoveUp => "Move up", "Lên";
    MoveDown => "Move down", "Xuống";
    Edit => "Edit", "Sửa";
    StepOf => "Step {0}/{1}", "Bước {0}/{1}";
    Plan => "Plan", "Kế hoạch";

    // W3-agent: session mode, side chat, memory, Design Jury, transcript
    SessionMode => "Session mode", "Chế độ phiên";
    ModeDesign => "Design", "Thiết kế";
    ModeChat => "Chat", "Hỏi đáp";
    ModePlan => "Plan", "Lập kế hoạch";
    ModeDesignHint => "Build and edit the design", "Dựng và sửa thiết kế";
    ModeChatHint => "Talk it through; no file changes", "Trao đổi, không sửa file";
    ModePlanHint => "Write a plan first, then build", "Viết kế hoạch trước rồi mới dựng";
    SideChat => "Side chat", "Chat phụ";
    SideChatHint => "Ask the same agent on the side; the main chat keeps its place.", "Hỏi cùng agent ở một luồng riêng, chat chính vẫn giữ nguyên.";
    SideChatPlaceholder => "Ask a quick question…  (Enter to send)", "Hỏi nhanh một câu…  (Enter để gửi)";
    Memory => "Memory", "Ghi nhớ";
    MemoryHint => "The agent reads these facts on every turn of this project. Start a message with “Remember that…” to add one from the chat.", "Agent đọc các ghi nhớ này ở mọi lượt của project. Mở đầu tin nhắn bằng “Ghi nhớ:” để thêm ngay từ chat.";
    MemoryEmpty => "Nothing remembered yet.", "Chưa có ghi nhớ nào.";
    MemoryPlaceholder => "Add a fact, e.g. Brand color is teal", "Thêm ghi nhớ, ví dụ: màu thương hiệu là xanh ngọc";
    MemoryAdd => "Add", "Thêm";
    MemoryClear => "Forget all", "Xoá hết";
    MemoryFromChat => "from chat", "từ chat";
    DesignJury => "Design Jury", "Hội đồng thiết kế";
    JuryHint => "Five reviewers score each draft; the agent revises until the composite clears 8.0.", "Năm giám khảo chấm từng bản nháp; agent sửa tới khi điểm tổng đạt 8.0.";
    JuryRound => "Round {0}", "Vòng {0}";
    JuryReviewing => "The jury is reviewing…", "Hội đồng đang chấm…";
    JuryShipped => "Shipped at round {0}, composite {1}", "Chốt ở vòng {0}, điểm tổng {1}";
    JuryBelow => "Below the threshold at round {0}, composite {1}", "Chưa đạt ngưỡng ở vòng {0}, điểm tổng {1}";
    JuryInterrupted => "Interrupted, best composite {0}", "Bị ngắt, điểm tổng cao nhất {0}";
    JuryDegraded => "The jury could not score this run ({0})", "Hội đồng không chấm được lượt này ({0})";
    JuryMustFix => "{0} to fix", "{0} chỗ phải sửa";
    JuryOpenArtifact => "Open the shipped file", "Mở file đã chốt";
    RoleDesigner => "Designer", "Thiết kế";
    RoleCritic => "Critic", "Phê bình";
    RoleBrand => "Brand", "Thương hiệu";
    RoleA11y => "Accessibility", "Trợ năng";
    RoleCopy => "Copy", "Nội dung";
    AskModeWarning => "{0} in “Ask first” mode can wait for an approval nobody can give and hang until the idle timeout. If a run stalls, switch Permissions to Automatic in Settings.", "{0} ở chế độ “Hỏi trước” có thể chờ một lần duyệt không ai duyệt được và treo tới hết idle timeout. Nếu run bị treo, đổi Quyền sang Tự động trong Cài đặt.";
    ExportConversation => "Export conversation", "Xuất hội thoại";
    ExportMarkdown => "Markdown (.md)", "Markdown (.md)";
    ExportJson => "JSON (.json)", "JSON (.json)";
}

/// The current language's text for `key`.
#[must_use]
pub fn t(key: T) -> &'static str {
    key.text(locale())
}

/// [`t`] with `{0}`, `{1}`… replaced by `args`.
#[must_use]
pub fn tf(key: T, args: &[&str]) -> String {
    fill(t(key), args)
}

fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_owned();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(s: &str) -> Vec<usize> {
        let mut v: Vec<usize> = (0..10)
            .filter(|i| s.contains(&format!("{{{i}}}")))
            .collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn every_key_is_translated_with_the_same_placeholders() {
        for key in T::ALL {
            let en = key.text(Locale::En);
            let vi = key.text(Locale::Vi);
            assert!(!en.trim().is_empty() && !vi.trim().is_empty(), "{key:?}");
            assert_eq!(placeholders(en), placeholders(vi), "{key:?}");
            assert_eq!(vi, vi.trim(), "{key:?} has stray whitespace");
        }
    }

    #[test]
    fn vietnamese_uses_new_style_tone_marks_and_no_ban() {
        // Kiểu mới (hoà, thuỷ, huỷ) throughout; impersonal UI, no "bạn".
        for key in T::ALL {
            let vi = key.text(Locale::Vi);
            for old in ["hóa", "hòa", "thủy", "hủy", "xóa", "khỏe", "lòa"] {
                assert!(!vi.to_lowercase().contains(old), "{key:?}: {vi}");
            }
            assert!(
                !vi.to_lowercase().split_whitespace().any(|w| w == "bạn"),
                "{key:?}: {vi}"
            );
        }
    }

    #[test]
    fn locale_parse_and_fill() {
        assert_eq!(Locale::parse("vi-VN"), Locale::Vi);
        assert_eq!(Locale::parse("VI"), Locale::Vi);
        assert_eq!(Locale::parse("en-US"), Locale::En);
        assert_eq!(Locale::parse(""), Locale::En);
        set_locale(Locale::Vi);
        assert_eq!(tf(T::StepOf, &["2", "5"]), "Bước 2/5");
        set_locale(Locale::En);
        assert_eq!(tf(T::StepOf, &["2", "5"]), "Step 2/5");
    }
}
