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

    // ---- Management screens (Projects, Settings, Automations, Integrations) ----
    // Projects page
    AnyType => "Any type", "Mọi loại";
    FilterByType => "Filter by type", "Lọc theo loại";
    SortBy => "Sort", "Sắp xếp";
    SortNewest => "Newest first", "Mới nhất trước";
    SortOldest => "Oldest first", "Cũ nhất trước";
    SortName => "Name", "Tên";
    ViewMode => "View mode", "Kiểu xem";
    ViewGrid => "Grid view", "Dạng lưới";
    ViewList => "List view", "Dạng danh sách";
    ViewBoard => "Board view", "Dạng board";
    ProjectCollections => "Collections", "Nhóm project";
    CollectionAll => "All projects", "Tất cả project";
    CollectionRecent => "Recently viewed", "Xem gần đây";
    SearchProjects => "Search projects and conversations", "Tìm project và conversation";
    SearchProjectsPlaceholder => "Search projects, conversations, messages…", "Tìm project, conversation, tin nhắn…";
    SearchResults => "Search results", "Kết quả tìm";
    HitProject => "Project", "Project";
    HitConversation => "Conversation", "Conversation";
    HitMessage => "Message", "Tin nhắn";
    UntitledConversation => "Untitled conversation", "Conversation chưa đặt tên";
    NoProjectsMatch => "No projects match your filters.", "Không có project nào khớp bộ lọc.";
    SelectMode => "Select", "Chọn";
    SelectProject => "Select project", "Chọn project";
    SelectedCount => "{0} selected", "Đã chọn {0}";
    DeleteSelected => "Delete selected", "Xoá mục đã chọn";
    ConfirmDeleteSelected => "Delete {0} project(s)?", "Xoá {0} project?";
    DeletedProjects => "{0} project(s) deleted.", "Đã xoá {0} project.";
    DeletedProjectsPartial => "Deleted {0} project(s); {1} failed.", "Đã xoá {0} project, {1} project lỗi.";
    StatusNotStarted => "Not started", "Chưa chạy";
    StatusRunning => "Running", "Đang chạy";
    StatusAwaitingInput => "Needs input", "Cần trả lời";
    StatusIncomplete => "Incomplete", "Chưa xong";
    StatusSucceeded => "Completed", "Hoàn tất";
    StatusFailed => "Failed", "Lỗi";
    StatusCanceled => "Canceled", "Đã huỷ";
    BoardEmptyColumn => "No designs", "Chưa có design";
    Freeform => "freeform", "tự do";
    TagPrototype => "Prototype", "Prototype";
    TagSlide => "Slide", "Slide";
    TagLiveArtifact => "Live artifact", "Live artifact";
    TagWebClone => "Website clone", "Clone website";
    TagMedia => "Media", "Media";
    TagOther => "Other", "Khác";
    LiveArtifactBadges => "Live artifact badges", "Nhãn live artifact";
    LiveCount => "{0} live", "{0} live";
    LiveRefreshing => "Refreshing…", "Đang làm mới…";
    LiveRefreshFailed => "Refresh failed", "Làm mới lỗi";
    LiveArchived => "Archived", "Đã lưu trữ";
    ImportZip => "Import ZIP", "Import ZIP";
    ImportZipTitle => "Import a Claude Design .zip export", "Import file .zip xuất từ Claude Design";
    ImportingZip => "Importing…", "Đang import…";
    ImportZipFailed => "Import failed: {0}", "Import lỗi: {0}";
    ProjectInstructions => "Project instructions", "Chỉ dẫn cho project";
    ProjectInstructionsHint => "Included in every message of this project. They win over the global rules in Settings.", "Gửi kèm mọi tin nhắn của project này. Khi mâu thuẫn, chỉ dẫn này thắng luật chung trong Cài đặt.";
    ProjectInstructionsPlaceholder => "Instructions for this project (injected on every message)…", "Chỉ dẫn cho project này (gửi kèm mọi tin nhắn)…";
    InstructionsActive => "Instructions active", "Đang dùng chỉ dẫn";
    // New-project panel
    NewProjectHint => "Pick what to make. The choices go to the agent with every turn.", "Chọn loại cần làm. Các lựa chọn này được gửi cho agent ở mọi lượt.";
    ProjectType => "Project type", "Loại project";
    ProjectName => "Name", "Tên";
    NoDesignSystem => "No design system", "Không dùng design system";
    TabPrototype => "Prototype", "Prototype";
    TabLiveArtifact => "Live artifact", "Live artifact";
    TabDeck => "Slide deck", "Slide deck";
    TabTemplate => "From template", "Từ template";
    TabMedia => "Media", "Media";
    TabOther => "Other", "Khác";
    TargetPlatforms => "Target platforms", "Nền tảng đích";
    PlatformResponsive => "Responsive web", "Web responsive";
    PlatformDesktopWeb => "Desktop web", "Web desktop";
    PlatformIos => "iOS app", "App iOS";
    PlatformAndroid => "Android app", "App Android";
    PlatformTablet => "Tablet app", "App tablet";
    PlatformDesktopApp => "Desktop app", "App desktop";
    CompanionSurfaces => "Companion surfaces", "Bề mặt đi kèm";
    IncludeLandingPage => "Include landing page", "Thêm landing page";
    IncludeOsWidgets => "Include OS widgets", "Thêm widget hệ điều hành";
    Fidelity => "Fidelity", "Độ chi tiết";
    FidelityWireframe => "Wireframe", "Wireframe";
    FidelityHigh => "High fidelity", "Chi tiết cao";
    UseSpeakerNotes => "Use speaker notes", "Dùng speaker notes";
    UseSpeakerNotesHint => "Less text on slides, keep talking points in notes.", "Ít chữ trên slide, ý nói để trong notes.";
    PickTemplate => "Pick a template", "Chọn template";
    IncludeAnimations => "Include animations", "Thêm animation";
    IncludeAnimationsHint => "Add motion (entrance, hover, transitions) on top of the template.", "Thêm chuyển động (xuất hiện, hover, chuyển cảnh) trên nền template.";
    MediaSurface => "Media type", "Loại media";
    SurfaceImage => "Image", "Ảnh";
    SurfaceVideo => "Video", "Video";
    SurfaceAudio => "Audio", "Âm thanh";
    MediaModel => "Model", "Model";
    Aspect => "Aspect", "Tỉ lệ";
    VideoLength => "Length", "Độ dài";
    Seconds => "{0}s", "{0} giây";
    AudioKind => "Audio type", "Loại âm thanh";
    AudioSpeech => "Speech / TTS", "Giọng nói / TTS";
    AudioSfx => "SFX", "Hiệu ứng âm thanh";
    Duration => "Duration", "Thời lượng";
    CreateLiveArtifact => "Create live artifact", "Tạo live artifact";
    CreateFromTemplate => "Create from template", "Tạo từ template";
    // Settings
    SettingsSections => "Settings sections", "Các mục cài đặt";
    SettingsModels => "Models & providers", "Model và provider";
    SettingsModelsHint => "Run a coding agent installed on this machine, or call a model provider with your own API key.", "Chạy agent CLI cài trên máy, hoặc gọi provider model bằng API key riêng.";
    ExecutionMode => "Execution mode", "Chế độ chạy";
    LocalCli => "Local CLI", "CLI trên máy";
    InstalledCount => "{0} installed", "Đã cài {0}";
    ApiProvider => "API provider", "Provider qua API";
    ModelCliDefault => "CLI default", "Mặc định của CLI";
    ModelSourceBuiltIn => "Built-in list", "Danh sách có sẵn";
    ModelSourceSuggested => "Suggested models", "Model gợi ý";
    ReasoningEffort => "Reasoning effort", "Mức suy luận";
    ServiceTier => "Service tier", "Gói dịch vụ";
    TierDefault => "Default", "Mặc định";
    TierFast => "Fast", "Nhanh";
    Test => "Test", "Kiểm tra";
    TestRunning => "Testing connection…", "Đang kiểm tra kết nối…";
    TestSuccessCli => "{0} replied in {1} ms: '{2}'", "{0} trả lời sau {1} ms: '{2}'";
    TestSuccessApi => "Connected. Replied in {0} ms: '{1}'", "Đã kết nối. Trả lời sau {0} ms: '{1}'";
    TestAuthFailed => "Authentication failed. Check the API key.", "Xác thực lỗi. Kiểm tra lại API key.";
    TestForbidden => "Access denied: {0}", "Bị từ chối: {0}";
    TestNotFoundModel => "Model '{0}' not found on this endpoint.", "Endpoint này không có model '{0}'.";
    TestInvalidBaseUrl => "Base URL is not usable: {0}", "Base URL không dùng được: {0}";
    TestRateLimited => "Rate limited. Try again in a moment.", "Bị giới hạn tần suất. Thử lại sau ít phút.";
    TestTimeout => "Test timed out after {0} ms.", "Hết giờ kiểm tra sau {0} ms.";
    TestAgentMissing => "{0} is not installed or not in PATH.", "{0} chưa cài hoặc không có trong PATH.";
    TestAgentSpawn => "Could not start {0}: {1}.", "Không chạy được {0}: {1}.";
    TestUnknown => "Test failed: {0}", "Kiểm tra lỗi: {0}";
    AdvancedEnv => "Advanced: proxy & custom paths", "Nâng cao: proxy và đường dẫn riêng";
    AdvancedEnvHint => "Extra environment for this agent's runs. Key values are stored on this machine only and never shown again.", "Biến môi trường thêm cho các run của agent này. Key chỉ lưu trên máy này và không hiện lại.";
    Provider => "Provider", "Provider";
    CustomProvider => "Custom provider", "Provider tự nhập";
    Protocol => "Protocol", "Giao thức";
    OpenAiCompatible => "OpenAI-compatible", "Tương thích OpenAI";
    BaseUrl => "Base URL", "Base URL";
    ApiKey => "API key", "API key";
    ApiKeyPlaceholder => "Paste the API key", "Dán API key";
    ApiKeyStored => "Saved key {0}; type to replace", "Đã lưu key {0}; gõ để thay";
    ApiKeyCleaned => "Removed spaces and hidden characters from the pasted key.", "Đã bỏ khoảng trắng và ký tự ẩn khỏi key vừa dán.";
    ApiKeyShape => "This key does not look like one for this provider.", "Key này có vẻ không phải của provider này.";
    ApiKeyLocalHint => "Kept in the data folder on this machine and sent only to this endpoint.", "Lưu trong thư mục dữ liệu trên máy này và chỉ gửi tới endpoint này.";
    ShowKey => "Show key", "Hiện key";
    HideKey => "Hide key", "Ẩn key";
    FetchModels => "Fetch models", "Lấy danh sách model";
    FetchedModels => "Fetched {0} models.", "Đã lấy {0} model.";
    SaveProvider => "Save and use", "Lưu và dùng";
    Notifications => "Notifications", "Thông báo";
    NotificationsHint => "Hear when a run ends, and get a desktop notification while the window is in the background.", "Nghe tiếng khi run xong, có thông báo desktop khi cửa sổ đang ở nền.";
    CompletionSound => "Completion sound", "Âm báo khi xong";
    SuccessSound => "Success", "Thành công";
    FailureSound => "Failure", "Lỗi";
    DesktopNotification => "Desktop notification", "Thông báo desktop";
    DesktopNotificationHint => "Sent when the window is not focused.", "Gửi khi cửa sổ không được focus.";
    NotificationsBlocked => "Notifications are blocked for this app in the system or browser settings.", "Thông báo đang bị chặn trong cài đặt hệ thống hoặc trình duyệt.";
    SendTest => "Send test", "Gửi thử";
    NotifyDone => "Task completed", "Đã xong việc";
    NotifyDoneBody => "A turn has finished.", "Một lượt vừa xong.";
    NotifyFailed => "Task failed", "Việc bị lỗi";
    NotifyFailedBody => "The task ended with an error.", "Việc kết thúc với lỗi.";
    NotifyTestBody => "Notifications work.", "Thông báo đã hoạt động.";
    ProjectLocations => "Project locations", "Nơi lưu project";
    ProjectLocationsHint => "Work bases that hold project folders. Each new project gets its own folder inside the default base.", "Các thư mục gốc chứa project. Project mới được lưu thành một thư mục bên trong thư mục mặc định.";
    BuiltInLocation => "Open Design projects", "Project của Open Design";
    DefaultLocation => "Default", "Mặc định";
    MakeDefault => "Make default", "Đặt làm mặc định";
    AddFolder => "Add folder", "Thêm thư mục";
    ScanLocations => "Scan for projects", "Quét tìm project";
    ScanDone => "Scan complete: {0} imported, {1} already registered.", "Quét xong: thêm {0}, {1} đã có.";
    InstructionsRules => "Instructions & rules", "Chỉ dẫn và luật";
    InstructionsRulesHint => "Rules the agent follows in every project. Project instructions win when both cover the same topic.", "Luật agent theo ở mọi project. Khi cùng một chủ đề, chỉ dẫn của project thắng.";
    GlobalRules => "Global rules", "Luật chung";
    GlobalRulesPlaceholder => "For example: Always use TypeScript. Prefer CSS grid for layouts.", "Ví dụ: Luôn dùng TypeScript. Ưu tiên CSS grid cho bố cục.";
    GlobalMemory => "Memory", "Bộ nhớ";
    GlobalMemoryHint => "Facts every project's agent should know. Each project also keeps its own memory in the Studio.", "Những điều agent ở mọi project cần biết. Mỗi project còn có bộ nhớ riêng trong Studio.";
    GlobalMemoryPlaceholder => "Add a fact, for example: The brand color is teal", "Thêm một điều, ví dụ: Màu thương hiệu là xanh teal";
    NoGlobalMemory => "Nothing remembered yet.", "Chưa ghi nhớ gì.";
    MemoryKind => "Memory type", "Loại ghi nhớ";
    MemoryUser => "User", "Người dùng";
    MemoryFeedback => "Feedback", "Góp ý";
    MemoryProject => "Project", "Project";
    MemoryReference => "Reference", "Tham khảo";
    MemoryProfile => "Profile", "Hồ sơ";
    MemoryRule => "Rule", "Luật";
    Add => "Add", "Thêm";
    MediaProviders => "Media providers", "Provider media";
    MediaProvidersHint => "Keys for image, video and audio generation. Stored on this machine.", "Key để tạo ảnh, video và âm thanh. Lưu trên máy này.";
    Configured => "Configured", "Đã cấu hình";
    Unset => "Not set", "Chưa đặt";
    ClearConfiguration => "Clear", "Xoá cấu hình";
    PasteApiKey => "Paste API key", "Dán API key";
    OverrideBaseUrl => "Override the default base URL", "Thay base URL mặc định";
    ProviderDefault => "Provider default", "Mặc định của provider";
    MediaSaveHint => "Changes apply after Save.", "Thay đổi có hiệu lực sau khi Lưu.";
    About => "About", "Giới thiệu";
    AboutHint => "Version, local diagnostics and first-run setup.", "Phiên bản, chẩn đoán cục bộ và thiết lập lần đầu.";
    Version => "Version", "Phiên bản";
    Runtime => "Runtime", "Môi trường chạy";
    RuntimePackaged => "Desktop app", "App desktop";
    RuntimeBrowser => "Browser", "Trình duyệt";
    Platform => "Platform", "Nền tảng";
    ExportDiagnostics => "Export diagnostics", "Xuất chẩn đoán";
    DiagnosticsHint => "A zip built on this machine: versions, settings without keys, detected agents and recent run logs. Nothing is uploaded.", "File zip tạo trên máy này: phiên bản, cài đặt không kèm key, agent tìm thấy và log run gần đây. Không gửi lên đâu.";
    DiagnosticsSaved => "Saved diagnostics to {0}", "Đã lưu chẩn đoán vào {0}";
    ResetOnboarding => "Reset onboarding", "Chạy lại thiết lập ban đầu";
    // Automations
    MetricActive => "Active", "Đang bật";
    MetricPaused => "Paused", "Tạm dừng";
    MetricTemplates => "Templates", "Template";
    EditRoutine => "Edit automation", "Sửa automation";
    UseTemplate => "Use template", "Dùng template";
    RoutineNamePlaceholder => "Morning briefing", "Tóm tắt buổi sáng";
    RoutinePromptPlaceholder => "Pull yesterday's activity and summarize what changed.", "Lấy hoạt động hôm qua và tóm tắt những gì đã đổi.";
    RoutineNeedsName => "Add a title for this automation.", "Cần đặt tên cho automation.";
    RoutineNeedsPrompt => "Add a prompt for the scheduled conversation.", "Cần nhập prompt cho conversation theo lịch.";
    TargetCreateEachRun => "New project each run", "Project mới mỗi lần chạy";
    TargetCreateEachRunHint => "A fresh, isolated workspace per fire.", "Mỗi lần chạy có một không gian riêng, mới.";
    TargetReuse => "Reuse an existing project", "Dùng lại một project";
    TargetReuseHint => "Each run lives as a new conversation inside the project.", "Mỗi lần chạy là một conversation mới trong project.";
    KindHourly => "Hourly", "Hằng giờ";
    KindDaily => "Daily", "Hằng ngày";
    KindWeekdays => "Weekdays", "Ngày làm việc";
    KindWeekly => "Weekly", "Hằng tuần";
    KindCustom => "Custom", "Tự nhập";
    FieldMinute => "Minute of every hour", "Phút của mỗi giờ";
    FieldTime => "Time", "Giờ";
    FieldTimezone => "Time zone", "Múi giờ";
    FieldWeekday => "Day of the week", "Ngày trong tuần";
    CronLine => "Cron line", "Dòng cron";
    DescribeHourly => "Runs every hour at :{0}", "Chạy mỗi giờ vào phút {0}";
    DescribeDaily => "Runs daily at {0} {1}", "Chạy hằng ngày lúc {0} {1}";
    DescribeWeekdays => "Runs Mon–Fri at {0} {1}", "Chạy thứ Hai đến thứ Sáu lúc {0} {1}";
    DescribeWeekly => "Runs every {0} at {1} {2}", "Chạy mỗi {0} lúc {1} {2}";
    Sunday => "Sunday", "Chủ nhật";
    Monday => "Monday", "Thứ Hai";
    Tuesday => "Tuesday", "Thứ Ba";
    Wednesday => "Wednesday", "Thứ Tư";
    Thursday => "Thursday", "Thứ Năm";
    Friday => "Friday", "Thứ Sáu";
    Saturday => "Saturday", "Thứ Bảy";
    RoutinePaused => "Paused", "Đang tạm dừng";
    ConfirmDeleteRoutine => "Delete this automation? Past runs and their projects are kept.", "Xoá automation này? Các lần chạy cũ và project của chúng vẫn giữ.";
    LastRunAt => "Last run {0}", "Lần chạy gần nhất {0}";
    ShowHistory => "History", "Lịch sử";
    HideHistory => "Hide history", "Ẩn lịch sử";
    InProgress => "In progress", "Đang chạy";
    TplMemoryRefresh => "Memory refresh", "Làm mới bộ nhớ";
    TplMemoryRefreshDesc => "Turn recent feedback into durable memory.", "Biến góp ý gần đây thành ghi nhớ lâu dài.";
    TplDesignSystem => "Design system maintainer", "Bảo trì design system";
    TplDesignSystemDesc => "Fold accepted patterns back into DESIGN.md.", "Đưa các mẫu đã duyệt vào DESIGN.md.";
    TplLiveArtifacts => "Live artifact maintainer", "Bảo trì live artifact";
    TplLiveArtifactsDesc => "Audit live artifacts and refresh stale versions.", "Rà live artifact và làm mới bản đã cũ.";
    TplStatusBoard => "Live status board", "Bảng trạng thái live";
    TplStatusBoardDesc => "Keep a live status artifact fresh.", "Giữ một artifact trạng thái luôn mới.";
    TplActivity => "Connector activity dashboard", "Dashboard hoạt động connector";
    TplActivityDesc => "A refreshable dashboard of recent activity.", "Dashboard hoạt động gần đây, làm mới được.";
    TplDigest => "Daily connector digest", "Tóm tắt connector hằng ngày";
    TplDigestDesc => "What changed in the last 24 hours.", "Những gì đã đổi trong 24 giờ qua.";
    TplReleaseNotes => "Weekly release notes", "Release notes hằng tuần";
    TplReleaseNotesDesc => "User-facing notes from the last 7 days.", "Ghi chú cho người dùng từ 7 ngày qua.";
    TplRegression => "Regression watch", "Theo dõi lỗi lùi";
    TplRegressionDesc => "Flag regressions against accepted work.", "Báo lỗi lùi so với bản đã duyệt.";
    // Integrations
    NavIntegrations => "Integrations", "Tích hợp";
    IntegrationsIntro => "External tools for the agent, and Open Design inside other coding agents.", "Công cụ ngoài cho agent, và dùng Open Design trong các agent khác.";
    ExternalTools => "External tools", "Công cụ ngoài";
    ConnectAgents => "Connect Open Design to your coding agent", "Kết nối Open Design với agent";
    ConnectAgentsShort => "Coding agents", "Agent lập trình";
    ConnectAgentsHint => "Register the Open Design MCP server in another agent so it can read and edit your designs.", "Đăng ký MCP server của Open Design trong agent khác để agent đó đọc và sửa design.";
    UseEverywhere => "Use everywhere", "Dùng ở mọi nơi";
    UseEverywhereShort => "CLI, HTTP, MCP", "CLI, HTTP, MCP";
    UseEverywhereHint => "Drive Open Design from a terminal, a script or another agent.", "Điều khiển Open Design từ terminal, script hoặc agent khác.";
    EverywhereCli => "CLI · od", "CLI · od";
    EverywhereCliBody => "Start the daemon, list projects, run an agent turn and export from a terminal.", "Chạy daemon, xem project, chạy một lượt agent và export từ terminal.";
    EverywhereMcp => "MCP server", "MCP server";
    EverywhereMcpBody => "Any MCP client can read projects and files through od mcp.", "Mọi MCP client đọc được project và file qua od mcp.";
    EverywhereHttp => "HTTP API", "HTTP API";
    EverywhereHttpBody => "The daemon answers on loopback; the UI uses the same API.", "Daemon trả lời trên loopback; UI cũng dùng API này.";
    EverywhereSkills => "Skills & headless", "Skill và chạy không giao diện";
    EverywhereSkillsBody => "Apply plugins and keep live artifacts fresh without the UI.", "Áp plugin và làm mới live artifact không cần UI.";
    CopyAgentGuide => "Copy guide for an agent", "Copy hướng dẫn cho agent";
    Copy => "Copy", "Copy";
    Copied => "Copied", "Đã copy";
    Client => "Client", "Client";
    MethodCommand => "Setup command", "Lệnh cài đặt";
    MethodToml => "TOML config", "Cấu hình TOML";
    MethodJson => "JSON config", "Cấu hình JSON";
    InstallInCursor => "Install in Cursor", "Cài vào Cursor";
    InstallNow => "Install", "Cài";
    Uninstall => "Remove", "Gỡ";
    Done => "Done", "Xong";
    McpLaunchNote => "Agents start the server with", "Agent khởi động server bằng";
    ForwardedTo => "Forwarded to:", "Được chuyển tới:";
    NotForwardedTo => "Not forwarded to:", "Không chuyển tới:";
    ForwardedAcpNote => "ACP adapters marked stdio only receive stdio MCP servers from this list; HTTP entries are dropped at spawn time.", "Adapter ACP ghi stdio only chỉ nhận MCP server stdio trong danh sách này; mục HTTP bị bỏ khi chạy.";
    NotForwardedNote => "For those agents, configure MCP servers in the agent's own config file (for example ~/.codex/config.toml); the servers below are unused there.", "Với các agent này, cấu hình MCP server trong file cấu hình riêng của agent (ví dụ ~/.codex/config.toml); các server bên dưới không được dùng ở đó.";
    StdioOnly => "{0} (stdio only)", "{0} (stdio only)";
    NoAgents => "no installed agent", "không có agent nào đã cài";
    McpCatalog => "Server templates", "Mẫu server";
    McpCatalogHint => "Ready-made servers. Adding one saves it to the list above; fill in keys or paths where a template asks.", "Server có sẵn. Thêm thì lưu vào danh sách trên; điền key hoặc đường dẫn nếu mẫu cần.";
    McpAdded => "Added {0}.", "Đã thêm {0}.";
    McpAlreadyAdded => "{0} is already in the list.", "{0} đã có trong danh sách.";
    GroupImageGeneration => "Image generation", "Tạo ảnh";
    GroupImageEditing => "Image editing", "Sửa ảnh";
    GroupWebCapture => "Web capture", "Chụp web";
    GroupDesignSystems => "Design systems", "Design system";
    GroupUiComponents => "UI components", "Component UI";
    GroupDataViz => "Data viz", "Biểu đồ dữ liệu";
    GroupPublishing => "Publishing", "Xuất bản";
    GroupUtilities => "Utilities", "Tiện ích";
    // Rail, message center, library
    NavDesignSystems => "Design systems", "Design system";
    NavPlugins => "Plugins", "Plugin";
    NavInbox => "Messages", "Tin báo";
    MessageCenter => "Message center", "Trung tâm tin báo";
    MessageCenterIntro => "Runs that finished, failed or were canceled, including those started by automations.", "Các run đã xong, lỗi hoặc bị huỷ, kể cả run do automation chạy.";
    Inbox => "Inbox", "Hộp thư";
    Archived => "Archived", "Đã lưu trữ";
    Archive => "Archive", "Lưu trữ";
    Unarchive => "Move back to inbox", "Đưa lại hộp thư";
    MarkAllRead => "Mark all read", "Đánh dấu đã đọc hết";
    NoMessages => "No messages yet", "Chưa có tin báo";
    NoMessagesBody => "Finished runs will appear here.", "Run xong sẽ hiện ở đây.";
    InboxRunIn => "in {0}", "trong {0}";
    DeletedProject => "a deleted project", "project đã xoá";
    OpenProject => "Open", "Mở";
    DesignSystemsIntro => "Installed design systems. Pick one when creating a project.", "Các design system đã cài. Chọn khi tạo project.";
    PluginsIntro => "Installed plugins. Apply one from the CLI or a project.", "Các plugin đã cài. Áp dụng từ CLI hoặc trong project.";
    PluginInputs => "{0} inputs", "{0} trường nhập";
    Search => "Search", "Tìm";
    SearchLibrary => "Filter by name or description", "Lọc theo tên hoặc mô tả";
    // Onboarding
    LocalAgent => "Local agent", "Agent trên máy";
    UseApiKey => "API key", "API key";
    LocalAgentIntro => "Use Claude Code, Codex, Cursor, OpenCode, Kimi, Qwen, Hermes, Kiro and more. The picked agent is tested once with a short prompt.", "Dùng Claude Code, Codex, Cursor, OpenCode, Kimi, Qwen, Hermes, Kiro… Agent được chọn sẽ được thử một lần bằng prompt ngắn.";
    ByokIntro => "Connect a model provider with your own key. Test it, then save.", "Kết nối provider model bằng key riêng. Kiểm tra rồi lưu.";
    RescanHint => "Rescan takes 5 to 10 seconds.", "Quét lại mất 5 đến 10 giây.";
    GateAgent => "Continue once the agent answered the test.", "Tiếp tục được khi agent đã trả lời bài thử.";
    GateByok => "Save a provider that passed the test to continue.", "Lưu một provider đã qua bài thử để tiếp tục.";
    // ---- end of management screens ----
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
    AutomationsIntro => "Run a prompt on a schedule, in one project or in a fresh project each time.", "Chạy một prompt theo lịch, trong một project hoặc trong project mới mỗi lần.";
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
