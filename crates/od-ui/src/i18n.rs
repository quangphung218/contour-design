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
    DaemonUnreachable => "Cannot reach the daemon. Check that the app is still running, then try again. ({0})", "Mất kết nối tới daemon. Kiểm tra app còn chạy rồi thử lại. ({0})";
    UnexpectedResponse => "Unexpected response: {0}", "Phản hồi không hợp lệ: {0}";
    ErrBadRequest => "The request was not accepted", "Yêu cầu chưa hợp lệ";
    ErrForbidden => "Not allowed", "Không có quyền";
    ErrNotFound => "Not found", "Không tìm thấy";
    ErrConflict => "Conflicts with a change in progress", "Đang vướng một thay đổi khác";
    ErrTooLarge => "Too large", "Dữ liệu quá lớn";
    ErrServer => "The daemon hit an error", "Daemon bị lỗi";

    // Home
    HomeTitle => "What do you want to design?", "Hôm nay thiết kế gì?";
    BriefPlaceholder => "Describe a page, an app screen or a deck…", "Mô tả một trang web, một màn hình app hay một deck…";
    OutputType => "Output type", "Loại đầu ra";
    ChipPrototype => "Prototype", "Prototype";
    ChipDeck => "Slide deck", "Slide deck";
    Create => "Create", "Tạo";
    Creating => "Creating…", "Đang tạo…";
    DesignSystem => "Design system", "Design system";
    NoneOption => "No design system", "Không dùng design system";
    NewDesignSystem => "New design system…", "Tạo design system…";
    DesignSystemNamePlaceholder => "Name, e.g. Tea House", "Tên, ví dụ Tea House";
    DesignSystemBriefPlaceholder => "Describe the brand: mood, colours, type, audience…", "Mô tả thương hiệu: không khí, màu, kiểu chữ, đối tượng…";
    GenerateDesignSystem => "Generate", "Tạo";
    GeneratingDesignSystem => "Generating…", "Đang tạo…";
    GeneratingDesignSystemHint => "The agent is writing DESIGN.md and tokens.css…", "Agent đang viết DESIGN.md và tokens.css…";
    DesignSystemCreated => "Created design system {0}.", "Đã tạo design system {0}.";
    DesignSystemFailed => "Could not create the design system: {0}", "Không tạo được design system: {0}";
    Template => "Template", "Template";
    DefaultTemplate => "Default template", "Template mặc định";
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
    OnboardingIntro => "Nét drives a coding agent installed on this machine. Nothing leaves your computer except what the agent itself sends to its provider.", "Nét điều khiển một coding agent cài sẵn trên máy. Không có dữ liệu nào rời khỏi máy, trừ những gì agent tự gửi tới provider của nó.";
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
    QuickOpen => "Go to file", "Mở file";
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
    OverridesDropped => "{0} unsaved style change(s) dropped: the element no longer reads as it did.", "Đã bỏ {0} thay đổi style chưa lưu: phần tử không còn như lúc chỉnh.";
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
    LintClean => "Lint", "Lint";
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
    // W3 platform: terminal tabs, automations, MCP servers
    NewTerminal => "New terminal", "Terminal mới";
    TerminalTab => "Terminal {0}", "Terminal {0}";
    TerminalExited => "[process exited with code {0}]", "[tiến trình đã thoát, mã {0}]";
    TerminalFailed => "Could not open a terminal: {0}", "Không mở được terminal: {0}";
    TerminalHint => "Click here and type. The shell runs in the project folder.", "Bấm vào đây rồi gõ. Shell chạy trong thư mục project.";
    CloseTerminal => "Close terminal", "Đóng terminal";
    NavAutomations => "Automations", "Tự động hoá";
    AutomationsIntro => "Run a prompt in a project on a schedule, in this computer's local time.", "Chạy một prompt trong project theo lịch, tính theo giờ máy.";
    RoutineName => "Name", "Tên";
    RoutineProject => "Project", "Project";
    RoutinePrompt => "Prompt", "Prompt";
    RoutineSchedule => "Schedule", "Lịch chạy";
    RoutineScheduleHint => "minute hour day month weekday, e.g. 0 9 * * 1-5, or @daily", "phút giờ ngày tháng thứ, ví dụ 0 9 * * 1-5, hoặc @daily";
    RoutineAdd => "Add routine", "Thêm routine";
    RoutineEnabled => "On", "Bật";
    RoutineRunNow => "Run now", "Chạy ngay";
    RoutineNextRun => "Next run: {0}", "Lần chạy tới: {0}";
    RoutineNotScheduled => "Not scheduled", "Chưa lên lịch";
    RoutineNoRuns => "No runs yet.", "Chưa chạy lần nào.";
    NoRoutines => "No routines yet.", "Chưa có routine nào.";
    RunStateRunning => "running", "đang chạy";
    RunStateSucceeded => "succeeded", "thành công";
    RunStateFailed => "failed", "lỗi";
    RunStateCanceled => "canceled", "đã huỷ";
    RunStateSkipped => "skipped: the previous run was still going", "bỏ qua: lần trước chưa xong";
    TriggerManual => "manual", "chạy tay";
    TriggerSchedule => "scheduled", "theo lịch";
    OpenConversation => "Open", "Mở";
    McpTitle => "MCP servers", "MCP server";
    McpIntro => "External MCP servers the agent can use. Keys and tokens stay on this computer.", "MCP server ngoài mà agent dùng được. Key và token chỉ lưu trên máy này.";
    McpAdd => "Add server", "Thêm server";
    McpId => "Id", "Id";
    McpCommand => "Command", "Lệnh";
    McpArgs => "Arguments, one per line", "Tham số, mỗi dòng một tham số";
    McpUrl => "URL", "URL";
    McpUseOauth => "Sign in with OAuth", "Đăng nhập bằng OAuth";
    McpTest => "Test", "Kiểm tra";
    McpTestOk => "Connected, {0} tools", "Đã kết nối, {0} tool";
    McpNeedsAuth => "Sign-in required", "Cần đăng nhập";
    McpConnect => "Sign in", "Đăng nhập";
    McpSignedIn => "Signed in", "Đã đăng nhập";
    McpSignOut => "Sign out", "Đăng xuất";
    McpFinishInBrowser => "Finish signing in in the browser, then test again.", "Đăng nhập xong trong trình duyệt rồi bấm Kiểm tra lại.";
    McpNoServers => "No MCP servers yet.", "Chưa có MCP server nào.";
    Remove => "Remove", "Gỡ";

    // Redesign: shell, home gallery, empty states, section hints
    MainNavigation => "Main navigation", "Điều hướng chính";
    CollapseSidebar => "Collapse sidebar", "Thu gọn thanh bên";
    ExpandSidebar => "Expand sidebar", "Mở rộng thanh bên";
    HomeSubtitle => "Describe it in a sentence. The agent builds a live prototype or deck you keep refining.", "Mô tả bằng một câu. Agent dựng prototype hoặc deck chạy được để chỉnh tiếp.";
    CreateHint => "Enter to create, Shift+Enter for a new line", "Enter để tạo, Shift+Enter để xuống dòng";
    TemplatesTitle => "Start from a template", "Bắt đầu từ template";
    TemplatesShowAll => "Show all {0}", "Xem cả {0}";
    TemplatesShowFewer => "Show fewer", "Thu gọn";
    TemplateSelected => "Selected", "Đang chọn";
    NoTemplates => "No templates for this output type yet.", "Chưa có template cho loại đầu ra này.";
    ProjectsEmptyTitle => "No projects yet", "Chưa có project nào";
    ProjectsEmptyBody => "Write a brief on Home, or open a folder you already have.", "Viết brief ở Trang chủ, hoặc mở một thư mục có sẵn.";
    MoreActions => "More actions", "Thêm thao tác";
    OtherAgents => "Not installed", "Chưa cài";
    AgentSectionHint => "The coding agent that runs briefs on this machine.", "Coding agent chạy brief trên máy này.";
    PermissionsHint => "What the agent may do without asking.", "Những việc agent được làm mà không cần hỏi.";
    AppearanceHint => "Follows the system unless you pin a theme.", "Theo hệ thống, trừ khi chọn cố định một chế độ.";
    LanguageHint => "Applies to the whole app after saving.", "Áp dụng cho cả app sau khi lưu.";
    NewRoutine => "New routine", "Routine mới";
    RoutinesTitle => "Routines", "Routine";
    NoRoutinesBody => "Add one above to run a prompt on a schedule.", "Thêm một routine ở trên để chạy prompt theo lịch.";
    PreviewEmptyTitle => "Nothing to preview yet", "Chưa có gì để preview";
    DescribeChangeTitle => "Start the conversation", "Bắt đầu trò chuyện";
    Files => "Files", "File";
    TurnOk => "done", "xong";
    TurnFailed => "failed", "lỗi";
    DsDialogTitle => "New design system", "Design system mới";
    DsDialogBody => "The agent writes DESIGN.md and tokens.css from this brief. The new system is selected when it is ready.", "Agent viết DESIGN.md và tokens.css theo mô tả này. Xong thì design system mới được chọn sẵn.";
    DsName => "Name", "Tên";
    DsBrief => "Brand brief", "Mô tả thương hiệu";

    // Brand
    Tagline => "Design with your agent, on your machine", "Thiết kế cùng agent, ngay trên máy";

    // Studio layout and preview toolbar
    HideFiles => "Hide files", "Ẩn danh sách file";
    ShowFiles => "Show files", "Hiện danh sách file";
    ToggleFilesPanel => "Show or hide files", "Hiện/ẩn danh sách file";
    NoFilesYetHint => "Files the agent writes show up here.", "File agent ghi ra sẽ hiện ở đây.";
    ViewAs => "View as", "Xem dạng";
    Code => "Code", "Code";
    TogglePreviewCode => "Switch preview and code", "Chuyển preview và code";
    CodeCut => "Showing the first {0} of {1} lines.", "Đang hiện {0} trên {1} dòng đầu.";
    Device => "Device", "Thiết bị";
    Zoom => "Zoom", "Thu phóng";
    ZoomIn => "Zoom in", "Phóng to";
    ZoomOut => "Zoom out", "Thu nhỏ";
    ZoomReset => "Reset zoom to 100%", "Về 100%";
    Annotate => "Annotate and inspect", "Chú thích và soi phần tử";
    PreviewWaitingTitle => "The agent is working", "Agent đang làm";
    PreviewWaiting => "The page shows here as soon as the first HTML file is written.", "Trang sẽ hiện ở đây ngay khi file HTML đầu tiên được ghi.";
    PreviewStep1 => "Describe what you want in the chat on the left.", "Mô tả thứ cần làm ở khung chat bên trái.";
    PreviewStep2 => "The agent writes the files; this preview reloads by itself.", "Agent ghi file; preview tự tải lại.";
    PreviewStep3 => "Comment on the page to ask for changes, then export.", "Bình luận lên trang để yêu cầu sửa, rồi xuất file.";

    // Run status and next steps
    ProjectLoadFailed => "This project could not be opened.", "Không mở được project này.";
    RunStarting => "Starting the agent…", "Đang khởi động agent…";
    RunWorking => "Working", "Đang chạy";
    RunStep => "Step {0} of {1}", "Bước {0}/{1}";
    RunDone => "Done", "Xong";
    RunFilesWritten => "{0} file(s) written", "Đã ghi {0} file";
    RunFailed => "Failed", "Lỗi";
    RunFailedHint => "See the message above, then retry or check the agent in Settings.", "Xem thông báo ở trên, rồi thử lại hoặc kiểm tra agent trong Cài đặt.";
    NextSteps => "Next", "Tiếp theo";
    NextOpen => "View {0}", "Xem {0}";
    NextComment => "Comment on the page", "Bình luận lên trang";
    NextMobile => "Mobile version", "Bản mobile";
    NextVariations => "Try variations", "Thử phương án khác";
    NextRetry => "Retry", "Thử lại";
    NextSettings => "Open Settings", "Mở Cài đặt";
    FollowupMobile => "Make it work well on a 390px phone screen.", "Chỉnh cho hiển thị tốt trên màn hình điện thoại 390px.";
    FollowupVariations => "Show me 3 different directions for this design.", "Đề xuất 3 hướng thiết kế khác nhau cho trang này.";
    StarterLanding => "A landing page for a tea shop", "Landing page cho một tiệm trà";
    StarterDashboard => "A sales dashboard with charts", "Dashboard doanh số có biểu đồ";
    StarterDeck => "A 6-slide pitch deck", "Pitch deck 6 slide";

    // Command palette and shortcuts
    CommandPalette => "Command palette", "Bảng lệnh";
    PaletteTrigger => "Search or run a command", "Tìm hoặc chạy lệnh";
    PalettePlaceholder => "Type a command, a page, a project or a file…", "Gõ tên lệnh, trang, project hoặc file…";
    PaletteEmpty => "Nothing matches \"{0}\".", "Không có gì khớp với \"{0}\".";
    PaletteActions => "Command", "Lệnh";
    PaletteStudio => "Studio", "Studio";
    PaletteMove => "move", "chọn";
    PaletteRun => "run", "chạy";
    ThemeLightCommand => "Theme: light", "Giao diện: sáng";
    ThemeDarkCommand => "Theme: dark", "Giao diện: tối";
    ThemeSystemCommand => "Theme: follow the system", "Giao diện: theo hệ thống";
    KeyboardShortcuts => "Keyboard shortcuts", "Phím tắt";
    ShortcutsGeneral => "Everywhere", "Mọi nơi";
    ShortcutsStudio => "Studio", "Studio";
    ShortcutsChat => "Chat", "Chat";
    ShortcutsDeck => "Slide deck", "Slide deck";
    ShortcutEsc => "Close a dialog, leave a mode", "Đóng hộp thoại, thoát chế độ";
    ShortcutPickTab => "Go to tab 1 to 9", "Sang tab 1 đến 9";
    ShortcutCycleTabs => "Next tab", "Tab kế tiếp";
    ShortcutZoom => "Zoom in, out, 100%", "Phóng to, thu nhỏ, 100%";
    ShortcutNewLine => "New line", "Xuống dòng";
    KeySpace => "Space", "Phím cách";
    ShortcutNextSlide => "Next slide", "Slide sau";
    ShortcutPrevSlide => "Previous slide", "Slide trước";
    ShortcutFirstLast => "First or last slide", "Slide đầu hoặc cuối";
    // Track B3-chat: Home composer, chat composer, transcript actions.
    HomeTitleCreate => "Let’s create {0}", "Cùng tạo {0}";
    HomeTitleWords => "prototypes,slide decks,documents", "prototype,slide deck,tài liệu";
    ChipWebClone => "Website clone", "Clone website";
    ChipWireframe => "Wireframe", "Wireframe";
    ChipMobileApp => "Mobile app", "App mobile";
    ChipDocument => "Document", "Tài liệu";
    ChipImage => "Image", "Ảnh";
    ChipVideo => "Video", "Video";
    ChipAudio => "Audio", "Âm thanh";
    ChipMotion => "Motion", "Motion";
    ChipHyperFrames => "HyperFrames", "HyperFrames";
    ChipWebGl => "WebGL", "WebGL";
    ChipLiveArtifact => "Live artifact", "Live artifact";
    ChipMore => "More", "Thêm";
    CarouselHint => "Attach a file, link your design system, or describe what you want to make", "Đính file, chọn design system, hoặc mô tả thứ cần làm";
    ScnOnePageBrief => "Draft a one-page project brief", "Soạn brief dự án một trang";
    ScnNotesToDeck => "Turn my notes into a presentation", "Biến ghi chú thành bài trình bày";
    ScnSignupFlow => "Mock up a signup flow", "Dựng luồng đăng ký";
    ScnImproveBrief => "Improve an existing project brief", "Làm tốt hơn một brief có sẵn";
    ScnLoadingAnimation => "Create a loading animation", "Làm hiệu ứng loading";
    ScnTeamUpdateSlides => "Design slides for a team update", "Làm slide cập nhật cho nhóm";
    ScnOrdersDashboard => "Prototype a dashboard for tracking orders", "Prototype dashboard theo dõi đơn hàng";
    ScnProductDetail => "Lay out a product detail page", "Dàn trang chi tiết sản phẩm";
    ScnCaseStudy => "Outline a case study", "Lên dàn ý case study";
    ScnLandingIntro => "Design a short intro for a landing page", "Làm phần mở đầu cho landing page";
    ScnPitchDeck => "Make a pitch deck for a new product", "Làm pitch deck cho sản phẩm mới";
    ScnAppIdea => "Describe an app idea", "Mô tả ý tưởng app";
    ScnLandingLayout => "Sketch a landing page layout", "Phác bố cục landing page";
    ExamplesTitle => "Try an example", "Thử một ví dụ";
    ExPrototype1 => "Design a high-converting website for an AI CRM with a clear hero, feature story, proof points, and trial CTA", "Thiết kế website cho một CRM dùng AI: hero rõ ràng, câu chuyện tính năng, bằng chứng và nút dùng thử";
    ExPrototype2 => "Create a desktop dashboard for a team knowledge base with search, recent updates, permissions, and collaboration entry points", "Làm dashboard desktop cho kho tri thức của nhóm: tìm kiếm, cập nhật gần đây, phân quyền và lối vào cộng tác";
    ExPrototype3 => "Redesign onboarding for a financial SaaS product so new users can connect data, finish setup, and see first value fast", "Thiết kế lại onboarding cho một SaaS tài chính để người mới kết nối dữ liệu, cài đặt xong và thấy giá trị nhanh";
    ExWebClone1 => "Website URL to clone: https://example.com", "URL website cần clone: https://example.com";
    ExWireframe1 => "Wireframe a SaaS dashboard with navigation, key metrics, a data table, and an empty state", "Wireframe dashboard SaaS: điều hướng, chỉ số chính, bảng dữ liệu và trạng thái trống";
    ExWireframe2 => "Sketch the lo-fi flow for a checkout: cart, shipping, payment, and confirmation screens", "Phác luồng thanh toán lo-fi: giỏ hàng, giao hàng, thanh toán và xác nhận";
    ExWireframe3 => "Wireframe an onboarding wizard covering account, workspace, invite, and first-run steps", "Wireframe wizard onboarding: tài khoản, workspace, mời người và lần chạy đầu";
    ExMobile1 => "Design a mobile banking app with home balance, transactions, transfer, and card management screens", "Thiết kế app ngân hàng: số dư, giao dịch, chuyển tiền và quản lý thẻ";
    ExMobile2 => "Lay out a food delivery app covering browse, restaurant detail, cart, and order tracking", "Dàn app giao đồ ăn: duyệt món, chi tiết quán, giỏ hàng và theo dõi đơn";
    ExMobile3 => "Prototype a habit-tracker app with today view, streak detail, add-habit, and stats screens", "Prototype app theo dõi thói quen: hôm nay, chuỗi ngày, thêm thói quen và thống kê";
    ExDocument1 => "Draft a one-page resume for a senior product designer with summary, experience, skills, and selected work", "Soạn CV một trang cho senior product designer: tóm tắt, kinh nghiệm, kỹ năng và dự án tiêu biểu";
    ExDocument2 => "Write a polished project proposal with problem, approach, timeline, and budget sections", "Viết đề xuất dự án gồm vấn đề, cách làm, tiến độ và ngân sách";
    ExDocument3 => "Generate a tidy meeting brief with agenda, decisions, owners, and next steps", "Tạo biên bản họp gọn: chương trình, quyết định, người phụ trách và việc tiếp theo";
    ExDeck1 => "Generate a weekly team status report with progress, risks, metric changes, and next-week priorities", "Tạo báo cáo tuần của nhóm: tiến độ, rủi ro, biến động chỉ số và ưu tiên tuần sau";
    ExDeck2 => "Design an investor pitch with market sizing, growth model, product advantage, and three-year forecast data", "Thiết kế pitch gọi vốn: quy mô thị trường, mô hình tăng trưởng, lợi thế sản phẩm và dự báo ba năm";
    ExDeck3 => "Create a strategic business review deck covering quarterly performance, root causes, opportunities, and next actions", "Làm deck đánh giá kinh doanh quý: kết quả, nguyên nhân, cơ hội và hành động tiếp theo";
    ExImage1 => "Generate a glassmorphism AI workspace poster with multi-screen collaboration, soft lighting, and a premium launch mood", "Tạo poster workspace AI kiểu glassmorphism: nhiều màn hình cộng tác, ánh sáng dịu, không khí ra mắt cao cấp";
    ExImage2 => "Create an ecommerce hero image for new wireless headphones that highlights material detail, lifestyle context, and core benefits", "Tạo ảnh hero ecommerce cho tai nghe không dây mới: chi tiết chất liệu, bối cảnh sử dụng và lợi ích chính";
    ExImage3 => "Design a minimalist tech launch key visual with a clean composition, strong product focus, and restrained launch copy", "Thiết kế key visual ra mắt công nghệ tối giản: bố cục sạch, tập trung vào sản phẩm, chữ tiết chế";
    ExVideo1 => "Make an 8-second product reveal film that moves from silhouette to close-up detail and ends on the brand mark", "Làm phim giới thiệu sản phẩm 8 giây: từ bóng mờ đến cận cảnh chi tiết, kết ở logo";
    ExVideo2 => "Create a vertical brand opener with rhythmic typography, product close-ups, and a clean logo ending for short-form video", "Làm đoạn mở đầu dọc cho video ngắn: chữ chuyển động theo nhịp, cận cảnh sản phẩm, kết bằng logo";
    ExVideo3 => "Turn a website into a 15-second social ad by extracting the hero claim, interaction highlights, and a clear CTA", "Biến một website thành quảng cáo mạng xã hội 15 giây: thông điệp chính, điểm nhấn tương tác và CTA rõ";
    ExMotion1 => "Create a 3-second logo outro using line convergence, subtle elasticity, and the brand color system", "Làm logo outro 3 giây: các nét hội tụ, độ nảy nhẹ và màu thương hiệu";
    ExMotion2 => "Make an animated flight-route map showing city nodes, route growth, mileage data, and a final summary frame", "Làm bản đồ đường bay chuyển động: các thành phố, tuyến bay mọc dần, số dặm và khung tổng kết";
    ExHyperFrames1 => "Build a captioned product launch short with title cards, feature shots, rhythmic transitions, and an ending CTA", "Dựng video ngắn ra mắt sản phẩm có phụ đề: thẻ tiêu đề, cảnh tính năng, chuyển cảnh theo nhịp và CTA cuối";
    ExHyperFrames2 => "Generate an audio-reactive data visualization where bars, particles, and titles respond to narration beats", "Tạo biểu đồ dữ liệu phản ứng theo âm thanh: cột, hạt và tiêu đề chạy theo nhịp lời đọc";
    ExAudio1 => "Generate a product startup sound that feels light, trustworthy, slightly futuristic, and suitable for a desktop app launch", "Tạo âm thanh khởi động sản phẩm: nhẹ, đáng tin, hơi tương lai, hợp cho app desktop";
    ExAudio2 => "Create a 20-second podcast intro bed with a warm opening, clear pulse, and a clean handoff into voiceover", "Làm nền nhạc mở đầu podcast 20 giây: mở ấm, nhịp rõ, chuyển gọn sang lời đọc";
    ExAudio3 => "Generate a branded notification sound set for success, reminder, and error states while keeping one sonic identity", "Tạo bộ âm báo cho thành công, nhắc việc và lỗi, cùng một bản sắc âm thanh";
    ExWebGl1 => "Build an interactive 3D hero with a slowly rotating product model, soft lighting, and scroll-driven camera moves", "Dựng hero 3D tương tác: mô hình sản phẩm xoay chậm, ánh sáng dịu, camera chạy theo cuộn trang";
    ExWebGl2 => "Create a particle field background that reacts to the cursor and settles into the brand logo", "Làm nền hạt phản ứng theo con trỏ rồi tụ lại thành logo";
    ExLive1 => "Make a live dashboard of this week's sign-ups, revenue, and churn that I can refresh", "Làm dashboard live về lượt đăng ký, doanh thu và churn tuần này, có thể làm mới";
    ExLive2 => "Build a live status page for our services with uptime, incidents, and response times", "Dựng trang trạng thái live cho các dịch vụ: uptime, sự cố và thời gian phản hồi";
    MediaRatio => "Ratio", "Tỉ lệ";
    MediaResolution => "Resolution", "Độ phân giải";
    MediaDuration => "Duration", "Thời lượng";
    MediaAudioKind => "Audio type", "Loại âm thanh";
    AudioSpeech => "Speech", "Giọng đọc";
    AudioSfx => "Sound effect", "Hiệu ứng âm thanh";
    SecondsN => "{0}s", "{0} giây";
    WorkingDir => "Working directory", "Thư mục làm việc";
    WorkingDirPick => "Choose folder", "Chọn thư mục";
    WorkingDirReplace => "Change working directory", "Đổi thư mục làm việc";
    WorkingDirRecent => "Recent folders", "Thư mục gần đây";
    WorkingDirClear => "Remove working directory", "Bỏ thư mục làm việc";
    WorkingDirHint => "Let the agent read this local folder (not imported into the project)", "Cho agent đọc thư mục này trên máy (không chép vào project)";
    WorkingDirUse => "Use folder", "Dùng thư mục";
    AddContext => "Add context", "Thêm ngữ cảnh";
    AttachFiles => "Attach files", "Đính file";
    Uploading => "Uploading…", "Đang tải lên…";
    UploadFailed => "Upload failed: {0}", "Tải lên lỗi: {0}";
    RemoveNamed => "Remove {0}", "Bỏ {0}";
    PreviewNamed => "Preview {0}", "Xem {0}";
    FolderNotAttachable => "Folders can't be attached here. Attach the files inside instead.", "Không đính được cả thư mục. Hãy đính các file bên trong.";
    TooManyFiles => "Attach up to {0} files at a time.", "Mỗi lần đính tối đa {0} file.";
    FileTooLarge => "{0} is larger than 200 MB.", "{0} lớn hơn 200 MB.";
    MenuPlugins => "Plugins", "Plugin";
    MenuSkills => "Skills", "Skill";
    MenuMcp => "MCP", "MCP";
    NoPluginsYet => "No installed plugins", "Chưa có plugin";
    NoSkillsYet => "No skills", "Chưa có skill";
    NoMcpYet => "No MCP servers", "Chưa có MCP server";
    AddMcpServer => "Add MCP server", "Thêm MCP server";
    ReferenceProject => "Reference another project", "Tham chiếu project khác";
    ReferenceProjectText => "Use the project “{0}” as a reference. Its files are in `{1}`; read them, do not change them.", "Dùng project “{0}” làm tham chiếu. File của nó nằm ở `{1}`; chỉ đọc, không sửa.";
    LinkLocalCode => "Link local code", "Liên kết code trên máy";
    LinkedFolders => "Linked folders", "Thư mục đã liên kết";
    Unlink => "Unlink {0}", "Bỏ liên kết {0}";
    FolderLinked => "Linked {0}: the agent can read it from the next message.", "Đã liên kết {0}: agent đọc được từ tin nhắn sau.";
    MentionEmpty => "Search files, plugins, skills and MCP servers.", "Tìm file, plugin, skill và MCP server.";
    MentionNoResults => "No results for “{0}”.", "Không có kết quả cho “{0}”.";
    MentionFiles => "Design files", "File thiết kế";
    MentionHint => "Up and down to move, Enter to pick, Esc to close", "Lên xuống để chọn, Enter để dùng, Esc để đóng";
    CommandsTitle => "Commands", "Lệnh";
    SlashMcp => "Open the MCP settings", "Mở cài đặt MCP";
    SlashMcpUse => "Use this server's tools", "Dùng tool của server này";
    SlashSearch => "Research a topic and save the notes", "Tìm hiểu một chủ đề và lưu ghi chú";
    SwitchModel => "Switch agent and model", "Đổi agent và model";
    ReasoningEffort => "Reasoning effort", "Mức suy luận";
    DefaultChoice => "Default", "Mặc định";
    CustomModel => "Custom model", "Model khác";
    NoAgentOnPath => "No agent detected on PATH", "Không tìm thấy agent nào trên PATH";
    ConversationActions => "Conversation actions", "Thao tác conversation";
    RenameConversation => "Rename conversation", "Đổi tên conversation";
    DeleteConversation => "Delete conversation", "Xoá conversation";
    DeleteConversationConfirm => "Delete “{0}”? This removes its messages.", "Xoá “{0}”? Mọi tin nhắn trong đó sẽ mất.";
    CollapseChat => "Collapse the conversation pane", "Thu gọn khung chat";
    ShowChat => "Show chat", "Hiện chat";
    ResizeChat => "Resize chat panel", "Đổi độ rộng khung chat";
    FilesFromTurn => "Files from this turn", "File của lượt này";
    WriteCount => "Write {0}", "Ghi {0}";
    EditCount => "Edit {0}", "Sửa {0}";
    OpenNamed => "Open {0}", "Mở {0}";
    DownloadNamed => "Download {0}", "Tải {0}";
    MoreN => "+{0} more", "+{0} file nữa";
    CopyMessage => "Copy", "Sao chép";
    CopiedMessage => "Copied", "Đã chép";
    ContinueRemaining => "Continue remaining tasks", "Làm tiếp phần còn lại";
    TaskNotFinished => "Task not finished yet: {0} item(s) left", "Việc chưa xong: còn {0} mục";
    Reconnect => "Reconnect", "Kết nối lại";
}

/// The current language's text for `key`.
#[must_use]
pub fn t(key: T) -> &'static str {
    key.text(locale())
}

/// Content text (template and skill descriptions, copied from the upstream
/// app) with the upstream product name swapped for Nét. The files keep
/// their original wording and attribution; only the UI shows the new name.
#[must_use]
pub fn rebrand(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains("OpenDesign") || text.contains("Open Design") {
        text.replace("OpenDesign", "Nét")
            .replace("Open Design", "Nét")
            .into()
    } else {
        text.into()
    }
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
    #[test]
    fn content_text_shows_the_new_name() {
        assert_eq!(
            rebrand("Create refreshable OpenDesign artifacts, made with Open Design."),
            "Create refreshable Nét artifacts, made with Nét."
        );
        assert!(matches!(
            rebrand("A tea shop"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

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
