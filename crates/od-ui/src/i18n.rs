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

    // Studio files, viewers, screenshots (B4)
    NewFileMenu => "New or upload", "Tạo mới hoặc tải lên";
    UploadFiles => "Upload files", "Tải file lên";
    PasteAsFile => "Paste as text file", "Dán thành file văn bản";
    CreateNew => "Create", "Tạo mới";
    NewDocument => "New document", "Tài liệu mới";
    NewDocumentHere => "New document here", "Tài liệu mới ở đây";
    NewPage => "New page", "Trang mới";
    NewSketch => "New sketch", "Sketch mới";
    NewFolder => "New folder", "Thư mục mới";
    NewFolderHere => "New folder here", "Thư mục mới ở đây";
    NewTab => "New tab", "Tab mới";
    OpenFileEllipsis => "Open file…", "Mở file…";
    SearchFiles => "Search files…", "Tìm file…";
    FilesViewAs => "Show files", "Cách hiện file";
    FilesViewTree => "Folders", "Theo thư mục";
    FilesViewKind => "By kind", "Theo loại";
    FilterByKind => "Filter by kind", "Lọc theo loại";
    FolderPath => "Folder path", "Đường dẫn thư mục";
    FolderUp => "Up one folder", "Lên thư mục cha";
    ProjectRoot => "Project", "Project";
    NoMatchingFiles => "No matching files", "Không có file khớp";
    SelectFile => "Select", "Chọn";
    RowMenu => "More actions", "Thao tác khác";
    OpenInTab => "Open in tab", "Mở trong tab";
    OpenInNewTab => "Open in new tab", "Mở trong tab mới";
    OpenThisFolder => "Open folder", "Mở thư mục";
    NotSaved => "Not saved", "Chưa lưu được";
    DownloadFile => "Download", "Tải xuống";
    DownloadZip => "Download as ZIP", "Tải xuống dạng ZIP";
    CopyPath => "Copy path", "Chép đường dẫn";
    CopyFullPath => "Copy full path", "Chép đường dẫn đầy đủ";
    PathCopied => "Copied {0}", "Đã chép {0}";
    MoveTo => "Move to folder…", "Chuyển vào thư mục…";
    Move => "Move", "Chuyển";
    MoveItems => "{0} item(s) will move.", "Sẽ chuyển {0} mục.";
    DestinationFolder => "Destination folder", "Thư mục đích";
    FilesMoved => "Moved {0} file(s) to {1}", "Đã chuyển {0} file vào {1}";
    RenameFile => "Rename file", "Đổi tên file";
    Name => "Name", "Tên";
    FileName => "File name", "Tên file";
    PastedText => "Text", "Nội dung";
    DeleteFiles => "Delete files", "Xoá file";
    ConfirmDeleteFiles => "Delete {0} file(s)? This cannot be undone.", "Xoá {0} file? Không hoàn tác được.";
    DeleteFolder => "Delete folder", "Xoá thư mục";
    ConfirmDeleteFolder => "Delete the folder {0} and everything in it? This cannot be undone.", "Xoá thư mục {0} và mọi thứ bên trong? Không hoàn tác được.";
    FilesDeleted => "Deleted {0} item(s)", "Đã xoá {0} mục";
    FolderCreated => "Created folder {0}", "Đã tạo thư mục {0}";
    FolderDeleted => "Deleted folder {0}", "Đã xoá thư mục {0}";
    FilesUploaded => "Uploaded {0} file(s)", "Đã tải lên {0} file";
    UploadFailed => "Upload failed: {0}", "Tải lên không thành công: {0}";
    NSelected => "{0} selected", "Đã chọn {0}";
    ClearSelection => "Clear selection", "Bỏ chọn";
    ShowMore => "Show {0} more", "Hiện thêm {0}";
    DropFilesTitle => "Drop files here", "Thả file vào đây";
    DropFilesHint => "Images, documents, references: the agent uses them as context.", "Ảnh, tài liệu, tham chiếu: agent dùng làm ngữ cảnh.";
    UntitledDocument => "Untitled document", "Tài liệu chưa đặt tên";
    UntitledPage => "Untitled page", "Trang chưa đặt tên";
    GroupPages => "Pages", "Trang";
    GroupDocuments => "Documents", "Tài liệu";
    GroupImages => "Images", "Ảnh";
    GroupSketches => "Sketches", "Sketch";
    GroupStylesheets => "Stylesheets", "Stylesheet";
    GroupScripts => "Scripts", "Script";
    GroupMedia => "Video and audio", "Video và âm thanh";
    GroupOther => "Other", "Khác";
    DocGoalHeading => "Goal", "Mục tiêu";
    DocGoalBody => "Describe what the agent should make, change or decide from this document.", "Mô tả agent cần tạo, sửa hay quyết định gì từ tài liệu này.";
    DocUseHeading => "How to use this document", "Cách dùng tài liệu này";
    DocUseMarkdown => "Write in Markdown: headings, lists, tables, checklists, links, quotes, code and images.", "Viết bằng Markdown: tiêu đề, danh sách, bảng, checklist, link, trích dẫn, code và ảnh.";
    DocUseAgent => "The agent can read and edit this file and uses it as project context.", "Agent đọc, sửa được file này và dùng làm ngữ cảnh của project.";
    DocUseMention => "Paste or drop images into the editor: they are uploaded and linked at the cursor.", "Dán hoặc thả ảnh vào trình soạn: ảnh được tải lên và chèn link tại con trỏ.";
    DocNextHeading => "Next step", "Bước tiếp theo";
    DocNextBody => "Edit this document, then ask the agent to build, revise or plan from it.", "Sửa tài liệu này rồi nhờ agent dựng, chỉnh hay lập kế hoạch từ nó.";
    MdViewMode => "Markdown view", "Chế độ xem Markdown";
    MdEditor => "Editor", "Soạn";
    MdSplit => "Split", "Chia đôi";
    MdPreview => "Preview", "Xem trước";
    MdPlaceholder => "Notes, requirements or instructions for this document…", "Ghi chú, yêu cầu hay hướng dẫn cho tài liệu này…";
    MdAutoSaveOn => "Auto-save on", "Đang bật tự lưu";
    MdAutoSaved => "Auto-saved at {0}", "Đã tự lưu lúc {0}";
    MdSaveFailed => "Save failed: {0}", "Lưu không thành công: {0}";
    Saving => "Saving…", "Đang lưu…";
    PreviewUnavailable => "No preview for this file", "File này không xem trước được";
    PreviewUnavailableHint => "Download it or open it in a new tab to look inside.", "Tải xuống hoặc mở trong tab mới để xem.";
    KindPdf => "PDF", "PDF";
    KindVideo => "Video", "Video";
    KindAudio => "Audio", "Âm thanh";
    KindWordDoc => "Document", "Tài liệu";
    KindSlidesDoc => "Presentation", "Bài trình chiếu";
    KindSheet => "Spreadsheet", "Bảng tính";
    KindBinary => "File", "File";
    DrawText => "Text", "Chữ";
    DrawTextPlaceholder => "Type, then Enter", "Gõ chữ rồi Enter";
    Undo => "Undo", "Hoàn tác";
    Redo => "Redo", "Làm lại";
    SketchToChat => "Send to chat", "Gửi vào chat";
    SketchAttached => "{0} goes with the next message", "{0} sẽ gửi kèm tin nhắn sau";
    Screenshot => "Screenshot", "Chụp màn hình";
    ScreenshotCopy => "Copy to clipboard", "Chép vào clipboard";
    ScreenshotToChat => "Send to chat", "Gửi vào chat";
    ScreenshotSave => "Save as PNG", "Lưu thành PNG";
    ScreenshotCopied => "Screenshot copied", "Đã chép ảnh chụp";
    ScreenshotCopyFailed => "Could not copy the screenshot: {0}", "Không chép được ảnh chụp: {0}";
    ScreenshotAttached => "Screenshot goes with the next message", "Ảnh chụp sẽ gửi kèm tin nhắn sau";
    ScreenshotFailed => "The preview did not answer the screenshot request. Reload the preview and try again.", "Preview không phản hồi lệnh chụp. Tải lại preview rồi thử lại.";
    FocusWorkspace => "Focus workspace (hide chat)", "Tập trung vùng làm việc (ẩn chat)";
    ShowChat => "Show chat", "Hiện chat";
    PresentOptions => "More ways to present", "Cách trình chiếu khác";
    PresentInTab => "In this tab", "Trong tab này";
    PresentFullscreen => "Fullscreen", "Toàn màn hình";
    PresentNewTab => "New tab", "Tab mới";
    PresenterView => "Presenter view", "Màn hình người trình bày";
    PresenterTitle => "Presenter: {0}", "Người trình bày: {0}";
    PresenterReset => "Reset", "Đặt lại";
    Elapsed => "Elapsed time", "Thời gian đã trình bày";
    CurrentSlide => "Current slide", "Slide hiện tại";
    EndOfDeck => "End of the deck", "Hết deck";
    EditNotes => "Edit", "Sửa";
    SaveNotes => "Save notes", "Lưu ghi chú";
    NotesPlaceholder => "Speaker notes for this slide…", "Ghi chú cho slide này…";
    NotesSaveFailed => "Could not save the notes: {0}", "Không lưu được ghi chú: {0}";
    ExportJpeg => "JPEG image", "Ảnh JPEG";
    ExportMp4 => "MP4 video (HyperFrames)", "Video MP4 (HyperFrames)";
    FileSection => "This file", "File này";
    SaveAsTemplate => "Save as template…", "Lưu thành template…";
    SaveTemplateBody => "The page, bundled into one file, joins the templates on Home.", "Trang này, gói thành một file, sẽ có trong danh sách template ở Trang chủ.";
    TemplateName => "Template name", "Tên template";
    TemplateDescription => "Description", "Mô tả";
    TemplateDescriptionHint => "What makes this template useful (optional)", "Template này hợp cho việc gì (không bắt buộc)";
    TemplateSaved => "Saved as the template {0}", "Đã lưu thành template {0}";
    HandoffChoose => "Hand off with options…", "Bàn giao có tuỳ chọn…";
    HandoffBody => "Pick the stack and where the work continues; the prompt is copied.", "Chọn stack và nơi làm tiếp; prompt sẽ được chép.";
    HandoffFramework => "Build with", "Dựng bằng";
    HandoffExistingStack => "The codebase's own stack", "Stack sẵn có của codebase";
    HandoffTarget => "Continue in", "Làm tiếp trong";
    HandoffPrompt => "Prompt", "Prompt";
    HandoffCopyOpenTerminal => "Copy prompt, open terminal", "Chép prompt, mở terminal";
    HandoffCopyOpenEditor => "Copy prompt, open {0}", "Chép prompt, mở {0}";
    HandoffCliReady => "Prompt copied: start {0} in the new terminal and paste it", "Đã chép prompt: chạy {0} trong terminal mới rồi dán vào";
    HandoffEditorOpened => "Prompt copied; opening {0}", "Đã chép prompt, đang mở {0}";
    ShortcutToggleSidebar => "Collapse or expand the sidebar (outside the Studio)", "Thu gọn/mở sidebar (ngoài Studio)";
    ShortcutPrevTab => "Previous tab", "Tab trước";
    ShortcutTabArrows => "Previous or next tab", "Tab trước hoặc sau";
    ShortcutContinueCli => "Continue in CLI (copy the hand-off prompt)", "Làm tiếp trong CLI (chép prompt bàn giao)";
    ShortcutSendComment => "Send the comment", "Gửi bình luận";
    ShortcutUndoDraw => "Undo, redo a drawing", "Hoàn tác, làm lại nét vẽ";
    ShortcutDeckReset => "Back to the first slide (and reset the presenter timer)", "Về slide đầu (đặt lại đồng hồ người trình bày)";
    ProjectActions => "Project actions", "Thao tác với project";
    DsFromProject => "Create design system from this project", "Tạo design system từ project này";
    DuplicateProject => "Duplicate project", "Nhân bản project";
    DsFromProjectRunning => "Creating the {0} design system…", "Đang tạo design system {0}…";
    DsFromProjectHint => "The agent reads this project's pages and styles. This takes a minute or two.", "Agent đọc các trang và style của project. Mất khoảng 1–2 phút.";
    DsFromProjectFailed => "Could not create the design system", "Không tạo được design system";
    DsReadyTitle => "{0} design system is ready", "Design system {0} đã sẵn sàng";
    DsReadyHint => "Automatic extraction can miss details: check it before using it widely. It is in the design system picker on Home.", "Trích tự động có thể sót chi tiết: xem lại trước khi dùng rộng. Design system có trong bộ chọn ở Trang chủ.";
    DsUseHere => "Use for this project", "Dùng cho project này";
    DsInUse => "This project now uses the new design system", "Project này đã dùng design system mới";
    Dismiss => "Dismiss", "Bỏ qua";
    SheetN => "Sheet {0}", "Trang tính {0}";
    OfficeNoText => "This file has no text to show.", "File này không có chữ để hiện.";
    ExportPptxEditable => "PowerPoint, editable (PPTX)", "PowerPoint chỉnh sửa được (PPTX)";
    Finalize => "Finalize design package (DESIGN.md)", "Chốt gói thiết kế (DESIGN.md)";
    Refinalize => "Re-finalize DESIGN.md", "Chốt lại DESIGN.md";
    RefinalizeStale => "Re-finalize DESIGN.md (older than the pages)", "Chốt lại DESIGN.md (cũ hơn các trang)";
    FinalizePrompt => "Finalize the design package: write DESIGN.md at the project root for a coding agent who will build this design. Cover the screens and their purpose, layout and grid, colors and type (exact values), spacing, components and their states, motion, copy and voice, and the assets used. Base it on the current pages and this conversation; keep it accurate and concise.", "Chốt gói thiết kế: viết DESIGN.md ở gốc project cho coding agent sẽ dựng thiết kế này. Gồm các màn hình và mục đích, bố cục và lưới, màu và chữ (giá trị chính xác), khoảng cách, component và các trạng thái, chuyển động, câu chữ và giọng văn, asset đang dùng. Dựa trên các trang hiện có và cuộc trò chuyện này; chính xác và ngắn gọn.";
    ContinueInCliCopied => "Prompt copied: paste it into a coding agent in the terminal", "Đã chép prompt: dán vào coding agent trong terminal";
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
