//! UI strings (spec 1.5 i18n): English and Vietnamese built in, the other
//! languages of the original app loaded on demand.
//!
//! Every visible string goes through [`t`] or [`tf`]. The table is one
//! macro call, so a key without both en and vi does not compile.
//! Vietnamese follows the vi-VN UI conventions: impersonal, kiểu mới tone
//! marks, product terms (agent, preview, deck, template…) kept in English.
//!
//! The other languages live in `locales/<tag>.txt` (`Key = text` lines),
//! served next to the bundle and fetched when picked, so they do not grow
//! the wasm. A key a file lacks falls back to English.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

macro_rules! locales {
    ($($variant:ident => $tag:literal, $name:literal;)*) => {
        /// A UI language.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
        pub enum Locale {
            #[default]
            $($variant),*
        }

        impl Locale {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];

            /// BCP 47 tag, also the value stored as `locale` in app settings.
            #[must_use]
            pub fn tag(self) -> &'static str {
                match self { $(Self::$variant => $tag),* }
            }

            /// The language's own name, for the picker.
            #[must_use]
            pub fn native_name(self) -> &'static str {
                match self { $(Self::$variant => $name),* }
            }
        }
    };
}

// Same set and order as the original app's `LOCALES`, plus Vietnamese.
locales! {
    En => "en", "English";
    Vi => "vi", "Tiếng Việt";
    Id => "id", "Bahasa Indonesia";
    De => "de", "Deutsch";
    ZhCn => "zh-CN", "简体中文";
    ZhTw => "zh-TW", "繁體中文";
    PtBr => "pt-BR", "Português (Brasil)";
    EsEs => "es-ES", "Español (España)";
    Ru => "ru", "Русский";
    Fa => "fa", "فارسی";
    Ar => "ar", "العربية";
    Ja => "ja", "日本語";
    Ko => "ko", "한국어";
    Pl => "pl", "Polski";
    Hu => "hu", "Magyar";
    Fr => "fr", "Français";
    Uk => "uk", "Українська";
    Tr => "tr", "Türkçe";
    Th => "th", "ภาษาไทย";
    It => "it", "Italiano";
}

impl Locale {
    /// Exact tag first, then Chinese by script or region, then the first
    /// language with the same primary subtag (`de-AT` → `de`, `pt` →
    /// `pt-BR`); anything else is English. Mirrors the original
    /// `resolveSystemLocale`.
    #[must_use]
    pub fn parse(tag: &str) -> Self {
        let tag = tag.trim();
        if let Some(exact) = Self::ALL.iter().find(|l| l.tag().eq_ignore_ascii_case(tag)) {
            return *exact;
        }
        let lower = tag.to_ascii_lowercase().replace('_', "-");
        let mut parts = lower.split('-');
        let language = parts.next().unwrap_or("");
        if language == "zh" {
            return match parts.next() {
                Some("hant" | "tw" | "hk" | "mo") => Self::ZhTw,
                _ => Self::ZhCn,
            };
        }
        Self::ALL
            .iter()
            .find(|l| l.tag().split('-').next() == Some(language))
            .copied()
            .unwrap_or(Self::En)
    }

    /// Whether the strings come from a `locales/<tag>.txt` file.
    #[must_use]
    pub fn is_loaded_on_demand(self) -> bool {
        !matches!(self, Self::En | Self::Vi)
    }

    /// Arabic and Persian run right to left (`<html dir>`).
    #[must_use]
    pub fn is_rtl(self) -> bool {
        matches!(self, Self::Ar | Self::Fa)
    }
}

thread_local! {
    static CURRENT: Cell<Locale> = const { Cell::new(Locale::En) };
    /// Parsed `locales/<tag>.txt` files, by locale; slots follow [`T::ALL`].
    static DICTS: RefCell<HashMap<Locale, Rc<[Option<&'static str>]>>> =
        RefCell::new(HashMap::new());
}

/// Sets the language [`t`] uses. The app re-renders after a change.
pub fn set_locale(locale: Locale) {
    CURRENT.with(|c| c.set(locale));
}

#[must_use]
pub fn locale() -> Locale {
    CURRENT.with(Cell::get)
}

/// Whether `locale`'s strings are ready ([`install_dict`] ran, or built in).
#[must_use]
pub fn has_dict(locale: Locale) -> bool {
    !locale.is_loaded_on_demand() || DICTS.with(|d| d.borrow().contains_key(&locale))
}

/// Parses a `locales/<tag>.txt` file and keeps it for `locale`. Returns
/// the lines that name no key (a stale file), for logging.
pub fn install_dict(locale: Locale, text: String) -> Vec<String> {
    // Loaded at most once per language for the page's life, so leaking
    // the text keeps every string `'static` like the built-in ones.
    let text: &'static str = Box::leak(text.into_boxed_str());
    let (slots, unknown) = parse_dict(text);
    DICTS.with(|d| d.borrow_mut().insert(locale, slots.into()));
    unknown
}

/// `Key = text` lines (`#` comments and blank lines skipped) into one slot
/// per [`T::ALL`] key, plus the keys that do not exist. `\n` in a text is
/// a line break.
fn parse_dict(text: &'static str) -> (Vec<Option<&'static str>>, Vec<String>) {
    let index: HashMap<&str, usize> = T::ALL
        .iter()
        .enumerate()
        .map(|(i, k)| (k.name(), i))
        .collect();
    let mut slots = vec![None; T::ALL.len()];
    let mut unknown = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            unknown.push(line.to_owned());
            continue;
        };
        let Some(&i) = index.get(key.trim()) else {
            unknown.push(key.trim().to_owned());
            continue;
        };
        let value = value.trim();
        slots[i] = Some(if value.contains("\\n") {
            &*Box::leak(value.replace("\\n", "\n").into_boxed_str())
        } else {
            value
        });
    }
    (slots, unknown)
}

fn lookup(locale: Locale, key: T) -> Option<&'static str> {
    DICTS.with(|d| {
        d.borrow()
            .get(&locale)
            .and_then(|slots| slots.get(key as usize).copied().flatten())
    })
}

macro_rules! messages {
    ($($key:ident => $en:literal, $vi:literal;)*) => {
        /// A UI string key.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum T { $($key),* }

        impl T {
            pub const ALL: &'static [T] = &[$(T::$key),*];

            /// The key's name, as written in `locales/<tag>.txt`.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self { $(T::$key => stringify!($key)),* }
            }

            /// The text in `locale`; English when that language's file
            /// is not loaded or lacks the key.
            #[must_use]
            pub fn text(self, locale: Locale) -> &'static str {
                match locale {
                    Locale::En => match self { $(T::$key => $en),* },
                    Locale::Vi => match self { $(T::$key => $vi),* },
                    other => lookup(other, self).unwrap_or_else(|| self.text(Locale::En)),
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
    PageNotFound => "Page not found", "Không tìm thấy trang";
    PageNotFoundBody => "The link may be old or mistyped.", "Link có thể đã cũ hoặc gõ sai.";
    GoToHome => "Go to Home", "Về trang chủ";
    JustNow => "just now", "vừa xong";
    MinAgo => "{0} min ago", "{0} phút trước";
    HoursAgo => "{0} h ago", "{0} giờ trước";
    DaysAgo => "{0} d ago", "{0} ngày trước";
    Loading => "Loading…", "Đang tải…";
    Cancel => "Cancel", "Huỷ";
    Save => "Save", "Lưu";
    SaveAgent => "Save agent", "Lưu agent";
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
    ErrNoWritePermission => "No permission to write", "Không có quyền ghi";
    ErrStorageFull => "Disk full", "Ổ đĩa đã đầy";

    // Home
    HomeTitle => "What do you want to design?", "Hôm nay thiết kế gì?";
    BriefPlaceholder => "Describe a page, an app screen or a deck…", "Mô tả một trang web, một màn hình app hay một deck…";
    BriefExample => "e.g. {0}", "VD: {0}";
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
    ProjectNameRequired => "A project name is required.", "Cần nhập tên project.";
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
    Seconds => "{0} s", "{0} giây";
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
    ApiKeyRequired => "API key required", "Cần API key";
    ApiKeyStored => "Saved key {0}; type to replace", "Đã lưu key {0}; gõ để thay";
    ApiKeyCleaned => "Removed spaces and hidden characters from the pasted key.", "Đã bỏ khoảng trắng và ký tự ẩn khỏi key vừa dán.";
    ApiKeyShape => "This key does not look like one for this provider.", "Key này có vẻ không phải của provider này.";
    ApiKeyLocalHint => "Kept in the data folder on this machine and sent only to this endpoint.", "Lưu trong thư mục dữ liệu trên máy này và chỉ gửi tới endpoint này.";
    ShowKey => "Show key", "Hiện key";
    HideKey => "Hide key", "Ẩn key";
    FetchModels => "Fetch models", "Lấy danh sách model";
    ApiVersion => "API version", "Phiên bản API";
    AzureDeployment => "Deployment", "Deployment";
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
    BuiltInLocation => "Contour projects", "Project của Contour";
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
    AboutHint => "Version, updates, local diagnostics and first-run setup.", "Phiên bản, bản cập nhật, chẩn đoán cục bộ và thiết lập lần đầu.";
    Version => "Version", "Phiên bản";
    Runtime => "Runtime", "Môi trường chạy";
    RuntimePackaged => "Desktop app", "App desktop";
    RuntimeBrowser => "Browser", "Trình duyệt";
    Platform => "Platform", "Nền tảng";
    ExportDiagnostics => "Export diagnostics", "Xuất chẩn đoán";
    DiagnosticsHint => "A zip built on this machine: versions, settings without keys, detected agents and recent run logs. Nothing is uploaded.", "File zip tạo trên máy này: phiên bản, cài đặt không kèm key, agent tìm thấy và log run gần đây. Không gửi lên đâu.";
    DiagnosticsSaved => "Saved diagnostics to {0}", "Đã lưu chẩn đoán vào {0}";
    ResetOnboarding => "Reset onboarding", "Chạy lại thiết lập ban đầu";
    YourUsage => "Your usage", "Số liệu sử dụng";
    YourUsageHint => "Worked out on this machine from your projects and runs, and never sent anywhere. Copy the summary to paste it into the “Share your usage” discussion on GitHub; for a bug, open an issue with steps to reproduce.", "Tính ngay trên máy này từ các project và run, không gửi đi đâu. Chép bản tóm tắt để dán vào discussion “Share your usage” trên GitHub; gặp bug thì mở issue kèm các bước tái hiện.";
    UsageFirstPreview => "Time to first preview", "Thời gian tới preview đầu tiên";
    UsageFirstPreviewValue => "{0} median · {1} projects", "Trung vị {0} · {1} project";
    UsageEngaged => "Projects exported or with 3+ chat turns", "Project đã export hoặc có từ 3 lượt chat";
    UsageEngagedValue => "{0}% · {1} of {2} projects", "{0}% · {1}/{2} project";
    UsageFailedRuns => "Runs that failed", "Run bị lỗi";
    UsageFailedRunsValue => "{0}% · {1} of {2} runs", "{0}% · {1}/{2} run";
    UsageNotEnoughProjects => "Needs {1} projects to measure (you have {0})", "Cần {1} project để tính (hiện có {0})";
    UsageNotEnoughRuns => "Needs {1} runs to measure (you have {0})", "Cần {1} run để tính (hiện có {0})";
    UsageSince => "Since {0}", "Từ bản {0}";
    UsageCopy => "Copy summary", "Chép tóm tắt";
    UsageCopied => "Summary copied. Paste it into the “Share your usage” discussion on GitHub.", "Đã chép tóm tắt. Dán vào discussion “Share your usage” trên GitHub.";
    UsageCopyFailed => "Couldn't copy. Select the summary below and copy it yourself.", "Không chép được. Chọn đoạn tóm tắt bên dưới rồi tự chép.";
    UsagePreview => "What Copy puts on the clipboard", "Nội dung sẽ được chép";
    CauseSpawnFailed => "Agent missing or didn't start", "Thiếu agent hoặc agent không chạy được";
    CauseAgentError => "Agent exited with an error", "Agent thoát vì lỗi";
    CauseTimeout => "Timed out", "Hết giờ";
    UsageStopped => "Stopped by you: {0} (not counted)", "Dừng tay: {0} run (không tính)";
    UsageSetup => "First-run setup", "Cài đặt lần đầu";
    UsageSetupReached => "Reached step {0} of {1}", "Đã tới bước {0}/{1}";
    UsageSetupTook => "{0} from first open to first preview", "{0} từ lần mở đầu tới preview đầu tiên";
    UsageSetupNext => "Next step: {0}", "Bước tiếp theo: {0}";
    UsageSetupNotRecorded => "Not recorded: this data folder is older than {0}", "Không có số liệu: thư mục dữ liệu có từ trước bản {0}";
    SetupStepOpened => "opening the app", "mở app";
    SetupStepAgent => "agent", "agent";
    SetupStepAgentTest => "agent test", "test agent";
    SetupStepProject => "first project", "project đầu tiên";
    SetupStepPreview => "first preview", "preview đầu tiên";
    CauseRateLimit => "Rate limited", "Bị giới hạn lượt gọi";
    CauseTransient => "Temporary provider error", "Lỗi tạm thời từ provider";
    CauseToolLoop => "Stuck repeating a tool", "Lặp mãi một tool";
    CauseDaemonRestart => "App restarted mid-run", "App khởi động lại giữa chừng";
    CauseOther => "Other", "Khác";
    CheckForUpdates => "Check for updates", "Kiểm tra bản cập nhật";
    CheckingForUpdates => "Checking for updates…", "Đang kiểm tra bản cập nhật…";
    UpdateAvailable => "Version {0} is available", "Đã có phiên bản {0}";
    UpdateYouHave => "You have version {0}.", "Bản đang dùng: {0}.";
    ViewRelease => "View release", "Xem bản phát hành";
    UpToDate => "You're up to date", "Đã là bản mới nhất";
    UpToDateHint => "Version {0} is the newest.", "Phiên bản {0} là bản mới nhất.";
    UpdateCheckFailed => "Couldn't reach GitHub. Try again in a few minutes.", "Không kết nối được GitHub. Thử lại sau ít phút.";
    UpdateRateLimited => "GitHub is limiting requests from this network. Try again in an hour.", "GitHub đang giới hạn lượt hỏi từ mạng này. Thử lại sau một giờ.";
    TryAgain => "Try again", "Thử lại";
    Licenses => "Licenses", "Giấy phép";
    LicensesHint => "Contour is licensed under the Apache License 2.0. These files ship with the app.", "Contour dùng giấy phép Apache License 2.0. Các file này đi kèm app.";
    Credits => "Credits", "Ghi công";
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
    TargetCreateEachRunHint => "A fresh, separate project for every run.", "Mỗi lần chạy có một project mới, riêng.";
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
    IntegrationsIntro => "External tools for the agent, and Contour inside other coding agents.", "Công cụ ngoài cho agent, và dùng Contour trong các agent khác.";
    ExternalTools => "External tools", "Công cụ ngoài";
    ConnectAgents => "Connect Contour to your coding agent", "Kết nối Contour với agent";
    ConnectAgentTab => "Connect an agent", "Kết nối agent";
    ConnectAgentsHint => "Register the Contour MCP server in another agent so it can read and edit your designs.", "Đăng ký MCP server của Contour trong agent khác để agent đó đọc và sửa design.";
    UseEverywhere => "Use everywhere", "Dùng ở mọi nơi";
    UseEverywhereShort => "CLI, HTTP, MCP", "CLI, HTTP, MCP";
    UseEverywhereHint => "Drive Contour from a terminal, a script or another agent.", "Điều khiển Contour từ terminal, script hoặc agent khác.";
    EverywhereCli => "CLI · od", "CLI · od";
    EverywhereCliBody => "Start the daemon, list projects, run an agent turn and export from a terminal.", "Chạy daemon, xem project, chạy một lượt agent và export từ terminal.";
    EverywhereMcp => "MCP server", "MCP server";
    EverywhereMcpBody => "Any MCP client can read projects and files through od mcp.", "Mọi MCP client đọc được project và file qua od mcp.";
    EverywhereHttp => "HTTP API", "HTTP API";
    EverywhereHttpBody => "The daemon answers on loopback; the UI uses the same API.", "Daemon trả lời trên loopback; UI cũng dùng API này.";
    EverywhereSkills => "Skills & headless", "Skill và chạy không giao diện";
    EverywhereSkillsBody => "Apply plugins and keep live artifacts fresh without the UI.", "Áp plugin và làm mới live artifact không cần UI.";
    CopyAgentGuide => "Copy guide for an agent", "Copy hướng dẫn cho agent";
    Copy => "Copy", "Sao chép";
    Copied => "Copied", "Đã sao chép";
    Client => "Client", "Client";
    MethodCommand => "Setup command", "Lệnh cài đặt";
    MethodToml => "TOML config", "Cấu hình TOML";
    MethodJson => "JSON config", "Cấu hình JSON";
    InstallInCursor => "Install in Cursor", "Cài vào Cursor";
    InstallNow => "Install", "Cài";
    Uninstall => "Remove", "Gỡ";
    McpLaunchNote => "Agents start the server with", "Agent khởi động server bằng";
    McpInstalling => "Updating the agent's settings…", "Đang cập nhật cài đặt của agent…";
    McpInstalledIn => "Contour is now available in {0}. Restart it to load the server.", "Đã thêm Contour vào {0}. Khởi động lại {0} để nạp server.";
    McpUninstalledIn => "Contour was removed from {0}. Restart it to unload the server.", "Đã gỡ Contour khỏi {0}. Khởi động lại {0} để gỡ server.";
    ForwardedTo => "Forwarded to:", "Được chuyển tới:";
    NotForwardedTo => "Not forwarded to:", "Không chuyển tới:";
    ForwardedAcpNote => "ACP adapters marked stdio only receive stdio MCP servers from this list; HTTP entries are dropped at spawn time.", "Adapter ACP ghi stdio only chỉ nhận MCP server stdio trong danh sách này; mục HTTP bị bỏ khi chạy.";
    NotForwardedNote => "For those agents, configure MCP servers in the agent's own config file (for example ~/.codex/config.toml); the servers below are unused there.", "Với các agent này, cấu hình MCP server trong file cấu hình riêng của agent (ví dụ ~/.codex/config.toml); các server bên dưới không được dùng ở đó.";
    StdioOnly => "{0} (stdio only)", "{0} (stdio only)";
    ForwardedDetails => "Details", "Chi tiết";
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
    MessageCenter => "Messages", "Tin báo";
    MessageCenterIntro => "Runs that finished, failed or were canceled, including those started by automations.", "Các run đã xong, lỗi hoặc bị huỷ, kể cả run do automation chạy.";
    Inbox => "Active", "Đang theo dõi";
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
    AgentsNoneHint => "No coding agent found yet. Paste an install command into Terminal, then choose Check again.", "Chưa có coding agent nào. Dán lệnh cài vào Terminal rồi bấm Kiểm tra lại.";
    InstallGuide => "Install guide", "Hướng dẫn cài";
    CopyCommand => "Copy command", "Sao chép lệnh";
    SignInAfterInstall => "Then open a new Terminal window and run {0} to sign in.", "Sau đó mở cửa sổ Terminal mới và chạy {0} để đăng nhập.";
    StillNoAgent => "Still no agent found. Open a new Terminal window and run {0} or {1}.", "Vẫn chưa thấy agent nào. Mở cửa sổ Terminal mới và chạy {0} hoặc {1}.";
    AgentTestFailedLead => "{0} did not pass its test. Update it, or install one of these instead.", "{0} chưa qua bài test. Cập nhật nó, hoặc cài một agent dưới đây.";
    AgentsNoneHintWindows => "No coding agent found yet. Install one from its guide below, or use an API key.", "Chưa có coding agent nào. Cài theo hướng dẫn bên dưới, hoặc dùng API key.";
    ClaudeAccount => "Needs a Claude Pro, Max, Team or Enterprise plan, or a Console account.", "Cần gói Claude Pro, Max, Team hoặc Enterprise, hoặc tài khoản Console.";
    CodexAccount => "Needs a ChatGPT plan or an OpenAI API key.", "Cần gói ChatGPT hoặc API key của OpenAI.";
    CheckAgain => "Check again", "Kiểm tra lại";
    CheckingAgents => "Checking…", "Đang kiểm tra…";
    ByokLimits => "API key only: makes a single HTML file, no tools.", "Chỉ API key: tạo một file HTML, không dùng tool.";
    UseApiKeyInstead => "Use an API key instead", "Dùng API key thay thế";
    ChromeNeeded => "PDF, PPTX and image export need Chrome.", "Xuất PDF, PPTX và ảnh cần Chrome.";
    GetChrome => "Get Chrome", "Tải Chrome";
    GateAgent => "Continue once the agent answered the test.", "Tiếp tục được khi agent đã trả lời bài thử.";
    GateByok => "Save a provider that passed the test to continue.", "Lưu một provider đã qua bài thử để tiếp tục.";
    // ---- end of management screens ----
    // Onboarding
    OnboardingTitle => "Choose your coding agent", "Chọn coding agent";
    OnboardingIntro => "Contour drives a coding agent installed on this machine. Nothing leaves your computer except what the agent itself sends to its provider.", "Contour điều khiển một coding agent cài sẵn trên máy. Không có dữ liệu nào rời khỏi máy, trừ những gì agent tự gửi tới provider của nó.";
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
    CommentMode => "Add comment", "Thêm bình luận";
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
    LintNone => "No design issues found.", "Không thấy lỗi thiết kế nào.";
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
    SendAttachedOnly => "Apply the attached comments and drawings.", "Áp dụng các bình luận và hình vẽ đính kèm.";
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
    ModeChat => "Ask", "Hỏi đáp";
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
    // Track B1-library: Design systems, Plugins/Skills, Media, Templates.
    NavMedia => "Media", "Media";
    NavTemplates => "Templates", "Template";
    AllFilter => "All", "Tất cả";
    AllCategories => "All categories", "Mọi nhóm";
    AllIndustries => "All industries", "Mọi ngành";
    AllProjectsFilter => "All projects", "Mọi project";
    Category => "Category", "Nhóm";
    PluginKindWorkflows => "Workflows", "Quy trình";
    PluginKindBlocks => "Building blocks", "Khối dựng";
    Surface => "Surface", "Bề mặt";
    SurfaceWeb => "Web", "Web";
    Source => "Source", "Nguồn";
    SourceLabel => "Source", "Nguồn";
    Accept => "Accept", "Nhận";
    Reject => "Reject", "Bỏ";
    CopyPrompt => "Copy prompt", "Chép prompt";
    Download => "Download", "Tải về";
    Upload => "Upload", "Tải lên";
    Uploaded => "Uploaded", "Đã tải lên";
    Browse => "Browse…", "Chọn…";
    Install => "Install", "Cài";
    Use => "Use", "Dùng";
    TryIt => "Try it", "Dùng thử";
    Previous => "Previous", "Trước";
    Next => "Next", "Sau";
    Failed => "Failed", "Thất bại";
    Description => "Description", "Mô tả";
    Prompt => "Prompt", "Prompt";
    Generate => "Generate", "Sinh";
    Fullscreen => "Fullscreen", "Toàn màn hình";
    OpenInNewTab => "Open in new tab", "Mở trong tab mới";
    ProjectLabel => "Project", "Project";
    ChooseProject => "Choose a project", "Chọn project";
    NoMatches => "Nothing matches the filters.", "Không có mục nào khớp bộ lọc.";
    ClearFilters => "Clear filters", "Xoá bộ lọc";
    SelectPlaceholder => "Select…", "Chọn…";
    SelectItem => "Select", "Chọn";
    ClearSelection => "Clear selection", "Bỏ chọn";
    ClearSearch => "Clear search", "Xoá tìm kiếm";
    DeleteNamed => "Delete {0}", "Xoá {0}";
    Kind => "Kind", "Loại";
    Desktop => "Desktop", "Desktop";
    Mobile => "Mobile", "Mobile";
    Industry => "Industry", "Ngành";
    DsPageSub => "Brand kits the agent designs with: browse the presets, create your own, and pick the default for new chats.", "Bộ nhận diện agent dùng để thiết kế: xem preset, tạo bộ riêng và chọn bộ mặc định cho chat mới.";
    DsYourSystems => "Your systems", "Của tôi";
    DsPresets => "Official presets", "Preset có sẵn";
    DsSearch => "Search design systems", "Tìm design system";
    DsEmptyMine => "No design systems yet. Create one from a brief, a website or a copy of a preset.", "Chưa có design system nào. Tạo từ mô tả, website hoặc bản sao của một preset.";
    DsNoMatch => "No design systems match the search.", "Không có design system nào khớp.";
    ChatDefault => "Chat default", "Mặc định";
    Published => "Published", "Đã xuất bản";
    Draft => "Draft", "Bản nháp";
    Publish => "Publish", "Xuất bản";
    MarkDraft => "Back to draft", "Chuyển về nháp";
    UseInNewProject => "Use in a new project", "Dùng cho project mới";
    EditWithAgent => "Edit with agent", "Sửa bằng agent";
    CopyToYourSystems => "Copy to your systems", "Chép về của tôi";
    DefaultForNewChats => "Default for new projects", "Mặc định cho project mới";
    DefaultNeedsPublished => "Publish this system first", "Cần xuất bản trước";
    DefaultSet => "New chats now start with this design system.", "Chat mới sẽ dùng design system này.";
    DefaultCleared => "No default design system now.", "Đã bỏ design system mặc định.";
    MoreActions => "More actions", "Thêm thao tác";
    DownloadZip => "Download ZIP", "Tải ZIP";
    DuplicateAction => "Duplicate", "Nhân bản";
    DsBrandNote => "Inspired by {0}'s public design language. Not affiliated with or endorsed by {0}. Trademarks belong to their owners.", "Lấy cảm hứng từ ngôn ngữ thiết kế công khai của {0}. Không liên kết với {0} và không được {0} bảo trợ. Nhãn hiệu thuộc về chủ sở hữu.";
    DsDeleteConfirm => "Delete the design system \"{0}\"? Its files are removed.", "Xoá design system \"{0}\"? Các file của nó sẽ bị xoá.";
    DsReviewTitle => "Review this design system", "Duyệt design system này";
    DsReviewBody => "Approve it to publish, or say what needs work and an agent revises it for you to accept.", "Duyệt để xuất bản, hoặc ghi chỗ cần sửa để agent sửa rồi chờ duyệt.";
    LooksGood => "Looks good", "Ổn rồi";
    NeedsWork => "Needs work…", "Cần sửa…";
    DsApproved => "Published.", "Đã xuất bản.";
    Section => "Section", "Mục";
    WholeSystem => "Whole system", "Toàn bộ";
    WhatNeedsWork => "What needs work", "Cần sửa gì";
    FeedbackPlaceholder => "Warmer greens, less contrast in buttons…", "Xanh lá ấm hơn, nút bớt tương phản…";
    ReviseWithAgent => "Revise with agent", "Nhờ agent sửa";
    RevisionStarted => "The agent is revising a copy. Review it when it is ready.", "Agent đang sửa một bản sao. Duyệt khi xong.";
    RevisionAccepted => "Revision applied.", "Đã áp dụng bản sửa.";
    RevisionRejected => "Revision discarded.", "Đã bỏ bản sửa.";
    RevisionFor => "Revision: {0}", "Bản sửa: {0}";
    TokenRebuildRevision => "Token contract rebuild", "Dựng lại token";
    AgentRevising => "Agent revising…", "Agent đang sửa…";
    AwaitingReview => "Awaiting review", "Chờ duyệt";
    ShowProposedChanges => "Show the proposed files", "Xem file đề xuất";
    KitTab => "Kit", "Kit";
    Showcase => "Showcase", "Showcase";
    Tokens => "Tokens", "Token";
    DesignMd => "DESIGN.md", "DESIGN.md";
    KitKeys => "C copy · R refresh", "C chép · R tải lại";
    KitKeysMine => "E edit · C copy · U upload · R rebuild · ⌫ delete asset", "E sửa · C chép · U tải lên · R dựng lại · ⌫ xoá asset";
    CopiedDesignMd => "DESIGN.md copied", "Đã chép DESIGN.md";
    TokensUpToDate => "Every token is bound; nothing to rebuild.", "Đủ token, không cần dựng lại.";
    RebuildProposed => "A rebuilt tokens.css is waiting for review.", "Bản tokens.css dựng lại đang chờ duyệt.";
    AssetsUploaded => "Uploaded.", "Đã tải lên.";
    AssetDeleted => "Asset deleted.", "Đã xoá asset.";
    LargePreview => "Large preview", "Xem lớn";
    Palette => "Palette", "Bảng màu";
    Typography => "Typography", "Kiểu chữ";
    TypeSample => "The quick brown fox jumps over the lazy dog.", "Chữ mẫu cho phần nội dung, đủ dấu tiếng Việt.";
    TokenContract => "Token contract", "Token contract";
    GradeExcellent => "Excellent", "Rất tốt";
    GradeUsable => "Usable", "Dùng được";
    GradeNeedsReview => "Needs review", "Cần xem lại";
    GradeNeedsRebuild => "Needs rebuild", "Cần dựng lại";
    MissingTokens => "{0} tokens fall back to defaults", "{0} token đang dùng giá trị mặc định";
    RebuildTokens => "Rebuild tokens", "Dựng lại token";
    BrandAssets => "Brand assets", "Asset thương hiệu";
    UploadLogoOrFont => "Upload logo or font", "Tải logo hoặc font";
    NoAssets => "This system ships no logos or fonts.", "Design system này không kèm logo hay font.";
    NoAssetsMine => "Add logos, images and fonts (U).", "Thêm logo, ảnh và font (U).";
    PreviewPages => "Preview pages", "Trang xem trước";
    DsCreateSub => "From a brief, a website, a repository, Figma, files, a pasted DESIGN.md or CSS, or a copy.", "Từ mô tả, website, repo, Figma, file, DESIGN.md hoặc CSS dán vào, hoặc bản sao.";
    SourceDescribe => "Describe", "Mô tả";
    SourceWebsite => "Website", "Website";
    SourceGithub => "GitHub", "GitHub";
    SourceFigma => "Figma", "Figma";
    SourceFiles => "Files", "File";
    SourcePaste => "Paste DESIGN.md", "Dán DESIGN.md";
    SourceCss => "CSS variables", "Biến CSS";
    SourceCopy => "Copy existing", "Chép bộ có sẵn";
    WebsiteUrls => "Website URLs, one per line", "URL website, mỗi dòng một URL";
    WebsiteUrlsPlaceholder => "https://example.com", "https://example.com";
    StartFromBrand => "Start from a brand", "Bắt đầu từ thương hiệu";
    BrandPickerSub => "The agent studies the brand's website.", "Agent sẽ đọc website của thương hiệu.";
    SearchBrands => "Search brands", "Tìm thương hiệu";
    GithubRepo => "GitHub repository", "Repo GitHub";
    GithubRepoPlaceholder => "owner/repo or https://github.com/owner/repo", "owner/repo hoặc https://github.com/owner/repo";
    FigmaUrl => "Figma file URL", "URL file Figma";
    FigmaUrlPlaceholder => "https://www.figma.com/file/…", "https://www.figma.com/file/…";
    FigmaHint => "The agent reads Figma through a Figma tool when it has one; add a website or files too for best results.", "Agent đọc Figma qua công cụ Figma nếu có; nên thêm website hoặc file để kết quả tốt hơn.";
    SourceFilesHint => "Screenshots, logos, CSS or theme files, up to 20 MB in all.", "Ảnh chụp, logo, file CSS hoặc theme, tổng tối đa 20 MB.";
    PasteDesignMdPlaceholder => "# Brand\n\n- **Primary** `#2f6b3a`\n…", "# Thương hiệu\n\n- **Chính** `#2f6b3a`\n…";
    PasteCssPlaceholder => ":root { --background: 0 0% 100%; --foreground: …; --primary: …; }", ":root { --background: 0 0% 100%; --foreground: …; --primary: …; }";
    CssHint => "A shadcn theme, a Tailwind @theme block or plain :root variables. Background, foreground and primary are required.", "Theme shadcn, khối @theme của Tailwind hoặc biến :root. Bắt buộc có background, foreground và primary.";
    CopyFrom => "Copy from", "Chép từ";
    ChooseDesignSystem => "Choose a design system", "Chọn design system";
    CopyOfName => "{0} copy", "Bản sao {0}";
    NotesOptional => "Notes (optional)", "Ghi chú (không bắt buộc)";
    NotesPlaceholder => "What to keep, what to change…", "Giữ gì, đổi gì…";
    PhaseSources => "Reading the sources", "Đọc nguồn";
    PhaseDesign => "Writing DESIGN.md", "Viết DESIGN.md";
    PhaseTokens => "Writing tokens.css", "Viết tokens.css";
    PhaseChecking => "Checking and installing", "Kiểm tra và cài";
    PluginsPageSub => "Expert workflows and skills the agent can run. Fill a plugin's inputs and use it, or make your own.", "Quy trình và skill agent chạy được. Điền input của plugin rồi dùng, hoặc tự tạo.";
    PluginsKind => "Plugins", "Plugin";
    SkillsKind => "Skills", "Skill";
    ScopeOfficial => "Official", "Chính thức";
    ScopePersonal => "Personal", "Cá nhân";
    SearchPlugins => "Search", "Tìm";
    PluginsEmptyPersonal => "Nothing here yet. Create a plugin or a skill.", "Chưa có gì. Hãy tạo plugin hoặc skill.";
    BackToList => "Back", "Quay lại";
    ByAuthor => "by {0}", "của {0}";
    UsePlugin => "Use plugin", "Dùng plugin";
    ExampleBrief => "Example brief", "Brief mẫu";
    Workflow => "Workflow", "Quy trình";
    ContextSkills => "Skills", "Skill";
    Craft => "Craft", "Craft";
    Atoms => "Atoms", "Atom";
    Capabilities => "Capabilities", "Quyền cần có";
    SourceBundled => "Bundled with the app.", "Đi kèm app.";
    SourceUserFolder => "Installed in your data folder.", "Cài trong thư mục dữ liệu.";
    DeveloperDetails => "Developer details", "Chi tiết cho nhà phát triển";
    UninstallConfirm => "Uninstall the plugin {0}?", "Gỡ plugin {0}?";
    VerbatimNotice => "Taken verbatim from this skill's SKILL.md.", "Lấy nguyên văn từ SKILL.md của skill.";
    OverrideNotice => "Editing a built-in skill saves a personal override; delete it to get the built-in back.", "Sửa skill có sẵn sẽ lưu một bản riêng; xoá bản riêng để dùng lại bản gốc.";
    DeleteOrReset => "Delete", "Xoá";
    SkillName => "Name", "Tên";
    Triggers => "Triggers", "Từ khoá kích hoạt";
    TriggersPlaceholder => "Comma separated: brand, voice", "Cách nhau bằng dấu phẩy: brand, voice";
    SkillBody => "Instructions (SKILL.md body)", "Hướng dẫn (thân SKILL.md)";
    CreateTitle => "Create", "Tạo mới";
    CreateWithAgent => "Plugin with agent", "Plugin bằng agent";
    InstallFromFolder => "Install from folder", "Cài từ thư mục";
    AddFromLink => "From link", "Từ link";
    AddFromLinkHint => "A GitHub repository or an archive link. A new project asks the agent to fetch it and install it; the app itself never downloads.", "Repo GitHub hoặc link file nén. Một project mới sẽ nhờ agent tải về và cài; app không tự tải gì.";
    PluginLink => "Plugin link", "Link plugin";
    PluginLinkPlaceholder => "github:owner/repo/path or https://…/plugin.zip", "github:owner/repo/path hoặc https://…/plugin.zip";
    FetchWithAgent => "Fetch with agent", "Nhờ agent tải";
    NewSkill => "New skill", "Skill mới";
    CreateWithAgentHint => "Describe the plugin. A new project asks the agent to write it and install it.", "Mô tả plugin. Một project mới sẽ nhờ agent viết và cài nó.";
    PluginBrief => "What should the plugin do?", "Plugin cần làm gì?";
    PluginBriefPlaceholder => "Turn a recipe into a printable card with a photo and steps", "Biến công thức nấu ăn thành thẻ in được có ảnh và các bước";
    StartWithAgent => "Start with agent", "Bắt đầu với agent";
    InstallFromFolderHint => "A folder with SKILL.md or open-design.json. It is copied into your plugins.", "Thư mục có SKILL.md hoặc open-design.json. Thư mục được chép vào plugin của tôi.";
    PluginPathPlaceholder => "/Users/me/plugins/recipe-cards", "/Users/me/plugins/recipe-cards";
    UsingSkill => "Skill: {0}", "Skill: {0}";
    MediaPageSub => "Every image, video and audio file across projects. Generate more with the providers you configured.", "Mọi ảnh, video và âm thanh trong các project. Sinh thêm bằng provider đã cấu hình.";
    GenerateMedia => "Generate", "Sinh media";
    Library => "Library", "Thư viện";
    MediaQueue => "Queue", "Hàng đợi";
    Providers => "Providers", "Provider";
    HyperframesMp4 => "Video to MP4", "Video ra MP4";
    SetUpProviders => "Set up providers", "Thiết lập nhà cung cấp";
    Images => "Images", "Ảnh";
    Videos => "Videos", "Video";
    Image => "Image", "Ảnh";
    Video => "Video", "Video";
    SearchMedia => "Search files", "Tìm file";
    ChatToDesign => "Chat to design", "Thiết kế từ đây";
    ChatToDesignPlaceholder => "What to design with them (optional)", "Thiết kế gì từ các file này (không bắt buộc)";
    DeleteAssetsConfirm => "Delete {0} files from their projects?", "Xoá {0} file khỏi project của chúng?";
    MediaEmptyTitle => "No media yet", "Chưa có media";
    MediaEmptyBody => "Upload files or generate an image, video or audio clip.", "Tải file lên hoặc sinh ảnh, video, âm thanh.";
    MediaPromptPlaceholder => "A ceramic teapot on linen, soft morning light", "Ấm trà gốm trên vải lanh, nắng sớm dịu";
    Ratio => "Ratio", "Tỉ lệ";
    Voice => "Voice", "Giọng";
    VoicePlaceholder => "ElevenLabs voice id (optional)", "Voice id ElevenLabs (không bắt buộc)";
    SaveTo => "Save to", "Lưu vào";
    MediaLibraryProject => "Media library", "Thư viện media";
    NoKey => "no key", "chưa có key";
    ProviderNeedsKey => "This model's provider has no API key yet.", "Provider của model này chưa có API key.";
    AddToQueue => "Add to queue", "Thêm vào hàng đợi";
    QueueEmpty => "Nothing queued.", "Hàng đợi trống.";
    TaskQueued => "queued", "đang chờ";
    TaskDone => "done", "xong";
    TaskInterrupted => "interrupted", "bị ngắt";
    ProvidersHint => "Keys stay on this machine (media-config.json, mode 0600). Leave a key empty to keep the saved one; an empty base URL uses the provider's own.", "Key chỉ lưu trên máy này (media-config.json, quyền 0600). Để trống key thì giữ key cũ; để trống base URL thì dùng địa chỉ mặc định của provider.";
    KeySaved => "Key saved ···{0}", "Đã lưu key ···{0}";
    KeyFromEnv => "Key from environment", "Key từ biến môi trường";
    Mp4Hint => "Renders a motion project to an MP4 saved in the project. Needs Chrome and ffmpeg.", "Render project chuyển động ra MP4 lưu trong project. Cần Chrome và ffmpeg.";
    FramesPerSecond => "Frames per second", "Khung hình mỗi giây";
    RenderMp4 => "Render MP4", "Render MP4";
    Mp4Saved => "Saved {0}.", "Đã lưu {0}.";
    TemplatesPageSub => "Start from a design template, a media prompt, or a project you saved.", "Bắt đầu từ design template, prompt media hoặc project đã lưu.";
    DesignTemplates => "Design templates", "Design template";
    ImagePrompts => "Image prompts", "Prompt ảnh";
    VideoPrompts => "Video prompts", "Prompt video";
    MyTemplates => "My templates", "Template của tôi";
    TemplateType => "Type", "Loại";
    SearchTemplates => "Search templates", "Tìm template";
    SearchPrompts => "Search prompts", "Tìm prompt";
    TemplatesCount => "{0} templates", "{0} template";
    NoExample => "This template has no rendered example.", "Template này chưa có bản mẫu.";
    DownloadHtml => "Download HTML", "Tải HTML";
    Remix => "Remix", "Remix";
    SaveProjectAsTemplate => "Save a project as a template", "Lưu project thành template";
    TemplateName => "Template name", "Tên template";
    SaveAsTemplate => "Save as template", "Lưu thành template";
    NoSavedTemplates => "No saved templates yet.", "Chưa có template nào được lưu.";
    FilesCount => "{0} files", "{0} file";
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
    TemplatesBrowseAll => "All templates ({0})", "Tất cả template ({0})";
    TemplateSelected => "Selected", "Đang chọn";
    NoTemplates => "No templates for this output type yet.", "Chưa có template cho loại đầu ra này.";
    ProjectsEmptyTitle => "No projects yet", "Chưa có project nào";
    ProjectsEmptyBody => "Write a brief on Home, or open a folder you already have.", "Viết brief ở Trang chủ, hoặc mở một thư mục có sẵn.";
    OtherAgentsCount => "Other agents ({0})", "Agent khác ({0})";
    AgentSectionHint => "The coding agent that runs briefs on this machine.", "Coding agent chạy brief trên máy này.";
    PermissionsHint => "What the agent may do without asking.", "Những việc agent được làm mà không cần hỏi.";
    AppearanceHint => "Follows the system unless you pin a theme.", "Theo hệ thống, trừ khi chọn cố định một chế độ.";
    LanguageHint => "Applies to the whole app.", "Áp dụng cho cả app.";
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
    OpenThisFolder => "Open folder", "Mở thư mục";
    NotSaved => "Not saved", "Chưa lưu được";
    DownloadFile => "Download", "Tải xuống";
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
    NotesSaveFailed => "Could not save the notes: {0}", "Không lưu được ghi chú: {0}";
    ExportJpeg => "JPEG image", "Ảnh JPEG";
    ExportWebp => "WebP image", "Ảnh WebP";
    ExportMp4 => "MP4 video", "Video MP4";
    ExportPowerPoint => "PowerPoint", "PowerPoint";
    ExportPptxSlides => "Slides as images", "Slide dạng ảnh";
    ExportPptxEditableShort => "Editable text", "Chữ chỉnh sửa được";
    FileSection => "This file", "File này";
    SaveTemplateBody => "The page, bundled into one file, joins the templates on Home.", "Trang này, gói thành một file, sẽ có trong danh sách template ở Trang chủ.";
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
    LiveArtifacts => "Live artifacts", "Live artifact";
    LiveData => "Data", "Dữ liệu";
    LiveHistory => "Refresh history", "Lịch sử làm mới";
    LiveUpToDate => "Up to date", "Đã cập nhật";
    LiveNotRefreshable => "Not refreshable", "Không làm mới được";
    LiveReady => "Ready to refresh", "Sẵn sàng làm mới";
    LiveRefreshed => "Refreshed: data and preview updated", "Đã làm mới: dữ liệu và preview đã cập nhật";
    LiveRefreshError => "Refresh failed: {0}", "Làm mới không thành công: {0}";
    LiveNoHistory => "No refreshes yet.", "Chưa làm mới lần nào.";
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
    ZoomFitsPane => "Fits the pane", "Đã vừa khung";
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
    ProjectGone => "This project was deleted or the link is wrong.", "Project này đã bị xoá hoặc link bị sai.";
    RunStarting => "Starting the agent…", "Đang khởi động agent…";
    RunWorking => "Working", "Đang chạy";
    RunStep => "Step {0} of {1}", "Bước {0}/{1}";
    RunDone => "Done", "Xong";
    RunFileWritten => "1 file written", "Đã ghi 1 file";
    RunFilesWritten => "{0} files written", "Đã ghi {0} file";
    RunFailed => "Failed", "Lỗi";
    RunFailedHint => "See the message above, then retry or check the agent in Settings.", "Xem thông báo ở trên, rồi thử lại hoặc kiểm tra agent trong Cài đặt.";
    RunAppClosed => "Contour closed while this turn was running, so it stopped partway.", "Contour đã đóng khi lượt này đang chạy nên lượt dừng giữa chừng.";
    RunAppClosedHint => "Retry to run it again.", "Bấm Thử lại để chạy lại lượt này.";
    NextSteps => "Next", "Tiếp theo";
    NextOpen => "View {0}", "Xem {0}";
    NextComment => "Comment on the page", "Bình luận lên trang";
    NextMobile => "Mobile version", "Bản mobile";
    NextVariations => "Try variations", "Thử phương án khác";
    NextRetry => "Retry", "Thử lại";
    NextSettings => "Open Settings", "Mở Cài đặt";
    AgentOutput => "Agent output", "Output của agent";
    AgentOutputEmpty => "The agent wrote nothing else before it stopped.", "Agent không ghi thêm gì trước khi dừng.";
    AgentOutputFailed => "Couldn't load the agent output.", "Không tải được output của agent.";
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
    // Track C3-runtimes: DeepSeek Harness connection setup.
    DshSetupTitle => "Set up DeepSeek Harness", "Thiết lập DeepSeek Harness";
    DshSetupDescription => "Contour runs the dsh CLI on this computer through its open-design profile. Install both, then detect again.", "Contour chạy dsh CLI trên máy này qua profile open-design. Cài cả hai rồi phát hiện lại.";
    DshStepCli => "1. Install the dsh CLI", "1. Cài dsh CLI";
    DshCliFound => "dsh found: {0}", "Đã thấy dsh: {0}";
    DshStepProfile => "2. Add the open-design profile", "2. Thêm profile open-design";
    DshStepProfileHint => "Build the profile package from the open-design source, then add it with dsh.", "Build gói profile từ mã nguồn open-design rồi thêm bằng dsh.";
    DshSetupNote => "DeepSeek Harness and its profile stay installed and managed by you. The guide has every step.", "DeepSeek Harness và profile do người dùng tự cài và quản lý. Hướng dẫn có đủ các bước.";
    DshSetupGuide => "Open the setup guide", "Mở hướng dẫn cài";
    DshSetupDetect => "Detect again", "Phát hiện lại";
    DshSetupDetecting => "Detecting…", "Đang phát hiện…";
    DshSetupNoCli => "dsh is still not found on PATH. Install it, or add its folder to PATH, then detect again.", "Vẫn chưa thấy dsh trên PATH. Cài dsh hoặc thêm thư mục của nó vào PATH rồi phát hiện lại.";
    DshSetupNoProfile => "The open-design profile is missing or does not pass the compatibility check yet.", "Profile open-design chưa có hoặc chưa qua bước kiểm tra tương thích.";
    DshSetupRequired => "Connection setup required", "Cần thiết lập kết nối";
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
    SecondsN => "{0}s", "{0} giây";
    WorkingDir => "Working directory", "Thư mục làm việc";
    WorkingDirPick => "Let the agent read a folder", "Cho agent đọc một thư mục";
    WorkingDirReplace => "Change working directory", "Đổi thư mục làm việc";
    WorkingDirRecent => "Recent folders", "Thư mục gần đây";
    WorkingDirClear => "Remove working directory", "Bỏ thư mục làm việc";
    WorkingDirHint => "Let the agent read this local folder (not imported into the project)", "Cho agent đọc thư mục này trên máy (không chép vào project)";
    WorkingDirUse => "Use folder", "Dùng thư mục";
    AddContext => "Add context", "Thêm ngữ cảnh";
    AttachFiles => "Attach files", "Đính file";
    Uploading => "Uploading…", "Đang tải lên…";
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
    MentionEmpty => "Search files, plugins, skills and MCP servers.", "Tìm file, plugin, skill và MCP server.";
    MentionNoResults => "No results for “{0}”.", "Không có kết quả cho “{0}”.";
    MentionFiles => "Design files", "File thiết kế";
    MentionHint => "Up and down to move, Enter to pick, Esc to close", "Lên xuống để chọn, Enter để dùng, Esc để đóng";
    CommandsTitle => "Commands", "Lệnh";
    SlashMcp => "Open the MCP settings", "Mở cài đặt MCP";
    SlashMcpUse => "Use this server's tools", "Dùng tool của server này";
    SlashSearch => "Research a topic and save the notes", "Tìm hiểu một chủ đề và lưu ghi chú";
    SwitchModel => "Switch agent and model", "Đổi agent và model";
    DefaultChoice => "Default", "Mặc định";
    CustomModel => "Custom model", "Model khác";
    NoAgentOnPath => "No agent detected on PATH", "Không tìm thấy agent nào trên PATH";
    ConversationActions => "Conversation actions", "Thao tác conversation";
    RenameConversation => "Rename conversation", "Đổi tên conversation";
    DeleteConversation => "Delete conversation", "Xoá conversation";
    DeleteConversationConfirm => "Delete “{0}”? This removes its messages.", "Xoá “{0}”? Mọi tin nhắn trong đó sẽ mất.";
    CollapseChat => "Collapse the conversation pane", "Thu gọn khung chat";
    ResizeChat => "Resize chat panel", "Đổi độ rộng khung chat";
    FilesFromTurn => "Files from this turn", "File của lượt này";
    WriteCount => "{0} new", "{0} mới";
    EditCount => "{0} edited", "{0} đã sửa";
    ToolPlan => "Updated plan", "Cập nhật kế hoạch";
    ToolWrote => "Wrote", "Đã ghi";
    ToolEdited => "Edited", "Đã sửa";
    ToolRead => "Read", "Đã đọc";
    ToolRan => "Ran", "Đã chạy";
    ToolSearched => "Searched", "Đã tìm";
    OpenNamed => "Open {0}", "Mở {0}";
    DownloadNamed => "Download {0}", "Tải {0}";
    MoreN => "+{0} more", "+{0} file nữa";
    CopyMessage => "Copy", "Sao chép";
    CopiedMessage => "Copied", "Đã chép";
    ContinueRemaining => "Continue remaining tasks", "Làm tiếp phần còn lại";
    TaskNotFinished => "Task not finished yet: {0} item(s) left", "Việc chưa xong: còn {0} mục";
    Reconnect => "Reconnect", "Kết nối lại";
    SearchList => "Search", "Tìm";
    OpWrite => "New", "Mới";
    OpEdit => "Edited", "Đã sửa";
    DesignToolbox => "Design toolbox", "Hộp công cụ thiết kế";
    ToolMatch => "Match next step", "Gợi ý bước tiếp theo";
    ToolMatchPrompt => "Define the taste bar for this design first, then pick the skills, files and tools that fit the next step and do it.", "Xác định chuẩn thẩm mỹ cho bản thiết kế này trước, rồi chọn skill, file và công cụ hợp với bước tiếp theo và làm luôn.";
    ToolIcons => "Find or replace icons", "Tìm hoặc thay icon";
    ToolIconsPrompt => "Find, generate or swap the icons on this page with one consistent style (SVG, Lucide-like line icons).", "Tìm, tạo hoặc thay icon trên trang này theo một phong cách thống nhất (SVG, icon nét kiểu Lucide).";
    ToolMotion => "Add animation", "Thêm chuyển động";
    ToolMotionPrompt => "Add entrance, scroll, state-transition and micro-interaction motion to the current page. Respect prefers-reduced-motion.", "Thêm chuyển động khi xuất hiện, khi cuộn, khi đổi trạng thái và tương tác nhỏ cho trang hiện tại. Tôn trọng prefers-reduced-motion.";
    ToolTaste => "Remove AI feel", "Bỏ cảm giác AI";
    ToolTastePrompt => "Remove the template feel: AI gradients, cheap card stacks, generic icons and vague copy. Keep the content.", "Bỏ cảm giác template: gradient kiểu AI, chồng card rẻ tiền, icon chung chung và chữ mơ hồ. Giữ nguyên nội dung.";
    ToolPolish => "Polish for shipping", "Hoàn thiện để giao";
    ToolPolishPrompt => "Audit visual hierarchy, type, spacing, responsive behaviour and accessibility, then fix what falls short.", "Rà thứ bậc thị giác, chữ, khoảng cách, responsive và khả năng truy cập, rồi sửa chỗ chưa đạt.";
    ToolImages => "Generate images", "Tạo ảnh";
    ToolImagesPrompt => "Generate section images, references or a moodboard for the current page and place them in it.", "Tạo ảnh cho các phần, ảnh tham khảo hoặc moodboard cho trang hiện tại và đặt vào trang.";
    ToolCharts => "Generate charts", "Tạo biểu đồ";
    ToolChartsPrompt => "Add charts or diagrams that explain the data on this page, styled to match it.", "Thêm biểu đồ hoặc sơ đồ giải thích dữ liệu trên trang này, cùng phong cách với trang.";
    Tool3d => "3D scene", "Cảnh 3D";
    Tool3dPrompt => "Add an interactive Three.js scene (particles, depth or a product model) where it strengthens the page.", "Thêm một cảnh Three.js tương tác (hạt, chiều sâu hoặc mô hình sản phẩm) ở chỗ làm trang mạnh hơn.";
    QuoteSelection => "Quote", "Trích dẫn";
    MediaNotes => "Style notes", "Ghi chú phong cách";
    CreatePlugin => "Create plugin", "Tạo plugin";
    CreatePluginPrompt => "Create a plugin for:", "Tạo một plugin cho:";
    FromFolder => "Open a folder as a project", "Mở thư mục thành project";
    StartFrom => "Start", "Bắt đầu";
    ShortcutMention => "Mention a file, plugin, skill or MCP server", "Nhắc tới file, plugin, skill hoặc MCP server";
    ShortcutSlash => "Commands (at the start of a line)", "Lệnh (ở đầu dòng)";
    // Track C1-preview: manual edit, comments tab, code colors, hints.
    FirstArtifactTitle => "Your first design is ready", "Bản thiết kế đầu tiên đã xong";
    FirstArtifactBody => "Edit and annotate it right in the preview, then export when you are happy.", "Sửa và chú thích ngay trên preview, ưng rồi thì export.";
    FirstArtifactDismiss => "Got it", "Đã hiểu";
    WorkspaceRecovering => "Connection to Contour interrupted. Reconnecting automatically…", "Mất kết nối tới Contour. Đang tự kết nối lại…";
    ToolManualEdit => "Edit element", "Sửa phần tử";
    ManualEditHint => "Click an element in the preview or pick a layer, then change it. Each change is saved as a version.", "Bấm một phần tử trên preview hoặc chọn một layer rồi sửa. Mỗi lần sửa được lưu thành một version.";
    CommentsTab => "All comments", "Mọi bình luận";
    CommentsFilterOpen => "Open", "Đang mở";
    CommentsFilterResolved => "Resolved", "Đã xong";
    CommentsNone => "No comments on this file yet. Turn on Comment and click an element.", "File này chưa có comment. Bật Comment rồi bấm vào một phần tử.";
    CommentsNoneResolved => "No resolved comments on this file.", "File này chưa có comment nào đã xong.";
    CommentStatusOpen => "Not sent", "Chưa gửi";
    CommentStatusAttached => "Goes with next message", "Gửi kèm tin sau";
    CommentStatusApplying => "Being applied", "Đang áp dụng";
    CommentStatusResolved => "Resolved", "Đã xong";
    CommentResolve => "Resolve", "Đánh dấu xong";
    CommentReopen => "Reopen", "Mở lại";
    AnchorStale => "Anchor may have moved", "Vị trí có thể đã đổi";
    AnchorLost => "Anchor lost", "Mất vị trí";
    CommentAttachImage => "Attach image", "Đính ảnh";
    CommentImageFailed => "Could not attach the image: {0}", "Không đính được ảnh: {0}";
    ManualLayers => "Layers", "Layer";
    ManualNoLayers => "No elements to edit on this page yet.", "Trang này chưa có phần tử nào để sửa.";
    ManualHidden => "Hidden", "Ẩn";
    ManualEmpty => "Click an element in the preview or pick a layer.", "Bấm một phần tử trên preview hoặc chọn một layer.";
    ManualMissing => "This element is no longer on the page.", "Phần tử này không còn trên trang.";
    ManualTabContent => "Content", "Nội dung";
    ManualTabStyle => "Style", "Kiểu";
    ManualTabAttributes => "Attributes", "Thuộc tính";
    ManualTabHtml => "Html", "Html";
    ManualText => "Text", "Chữ";
    ManualHref => "Link (href)", "Liên kết (href)";
    ManualImageUrl => "Image URL", "URL ảnh";
    ManualAlt => "Alt text", "Mô tả ảnh (alt)";
    ManualUploadImage => "Upload image", "Tải ảnh lên";
    ManualUploadFailed => "Could not upload the image: {0}", "Không tải được ảnh lên: {0}";
    ManualContainerHint => "This element holds other elements. Edit its markup in the Html tab, or pick a layer inside it.", "Phần tử này chứa phần tử khác. Sửa markup ở tab Html, hoặc chọn một layer bên trong.";
    ManualAttributesJson => "Attributes (JSON)", "Thuộc tính (JSON)";
    ManualSelectedHtml => "Element HTML (as in the source)", "HTML của phần tử (như trong source)";
    ManualInvalidAttributes => "Invalid attributes JSON: {0}", "JSON thuộc tính không hợp lệ: {0}";
    ManualApply => "Apply", "Áp dụng";
    ManualApplyContent => "Content", "Nội dung";
    ManualLeaveTitle => "Leave this page?", "Rời trang này?";
    ManualLeave => "Leave", "Rời trang";
    ManualLeaveConfirm => "An edit is not applied yet. Leave and lose it?", "Có chỉnh sửa chưa áp dụng. Rời trang và bỏ chỉnh sửa này?";
    ManualApplyStyle => "Style", "Kiểu";
    ManualApplyAttributes => "Attributes", "Thuộc tính";
    ManualApplyHtml => "HTML", "HTML";
    ManualDelete => "Delete element", "Xoá phần tử";
    ManualUndo => "Undo", "Hoàn tác";
    ManualRedo => "Redo", "Làm lại";
    ManualHistoryFailed => "Could not undo or redo: {0}", "Không hoàn tác hoặc làm lại được: {0}";
    // ---- end of track C1-preview ----
    // Track C2-chat-home: Settings memory, memory toast, library import,
    // onboarding brand step, Design Jury, media rows, Home, quick menu, help.
    HookProfile => "Use my profile", "Dùng hồ sơ của tôi";
    HookProfileHint => "Inject your work profile into every task.", "Đưa hồ sơ công việc vào mọi tác vụ.";
    HookRewrite => "Rewrite short asks", "Viết lại yêu cầu ngắn";
    HookRewriteHint => "Before: expand a short request into a full task brief.", "Trước: mở rộng yêu cầu ngắn thành brief đầy đủ.";
    HookVerify => "Self-verify output", "Tự kiểm kết quả";
    HookVerifyHint => "After: check the result against your rules, then show a scorecard.", "Sau: đối chiếu kết quả với các quy tắc, rồi hiện bảng điểm.";
    HookLearn => "Learn from chats", "Học từ chat";
    HookLearnHint => "Capture useful facts and preferences from chat messages.", "Ghi lại sự thật và sở thích hữu ích từ tin nhắn chat.";
    MemoryEnable => "Enable memory injection", "Bật đưa bộ nhớ vào prompt";
    MemoryDisabled => "Memory is off.", "Bộ nhớ đang tắt.";
    MemoryDisabledBody => "Existing facts stay on disk but are not added to new chats, and new turns learn nothing.", "Các mục đã có vẫn nằm trên đĩa nhưng không được đưa vào chat mới, và lượt mới không học thêm gì.";
    MemoryHooks => "Memory hooks", "Memory hooks";
    MemoryHooksHint => "Choose which parts of memory run on their own. The switch above turns all of them off.", "Chọn phần nào của bộ nhớ tự chạy. Công tắc phía trên tắt tất cả.";
    MemoryImport => "Import", "Nhập";
    MemoryEditIndex => "Edit MEMORY.md", "Sửa MEMORY.md";
    MemoryImportHint => "Paste or pick a Markdown file (a MEMORY.md, CLAUDE.md or notes). Bullets become facts; headings such as Rules or Profile set their type.", "Dán hoặc chọn file Markdown (MEMORY.md, CLAUDE.md hay ghi chú). Mỗi gạch đầu dòng thành một mục; tiêu đề như Rules hay Profile quyết định loại.";
    MemoryImportPlaceholder => "- Prefers serif headings", "- Thích tiêu đề chữ có chân";
    MemoryImportFile => "Choose file", "Chọn file";
    MemoryImportScan => "Find facts", "Tìm mục";
    MemoryImportNone => "No facts found. Use one bullet per fact.", "Không tìm thấy mục nào. Mỗi mục là một gạch đầu dòng.";
    MemorySuggested => "Suggested memories", "Mục gợi ý";
    MemorySaveSelected => "Save selected", "Lưu mục đã chọn";
    MemoryImported => "Saved {0} memories.", "Đã lưu {0} mục.";
    MemoryImportPartial => "Saved {0}, {1} failed.", "Đã lưu {0}, {1} mục lỗi.";
    MemoryIndexTitle => "MEMORY.md (index)", "MEMORY.md (mục lục)";
    MemoryIndexHint => "Only facts linked here reach the prompt. Delete a line to turn a fact off; the entry stays in the list. Not saved until Save index.", "Chỉ mục có liên kết ở đây mới vào prompt. Xoá một dòng để tắt mục đó; mục vẫn còn trong danh sách. Chỉ lưu khi bấm Lưu mục lục.";
    MemoryIndexSave => "Save index", "Lưu mục lục";
    MemoryIndexSaved => "Index saved", "Đã lưu mục lục";
    MemoryUnsaved => "Unsaved changes", "Chưa lưu";
    MemoryIndexReset => "Reset", "Đặt lại";
    MemoryDiscard => "Discard", "Bỏ";
    MemoryToastGlobal => "Memory updated ({0})", "Đã cập nhật bộ nhớ ({0})";
    MemoryToastProject => "Saved to project memory ({0})", "Đã lưu vào bộ nhớ project ({0})";
    MemoryToastView => "View", "Xem";
    ImportFromLibrary => "Import from library", "Nhập từ thư viện";
    LibrarySearch => "Search library", "Tìm trong thư viện";
    LibraryFilterKind => "Kind", "Loại";
    LibraryFilterAll => "All", "Tất cả";
    LibraryEmpty => "No assets in your library yet", "Thư viện chưa có gì";
    LibraryImportFailed => "Could not add from library ({0})", "Không thêm được từ thư viện ({0})";
    LibraryImport => "Import", "Nhập";
    LibraryImportCount => "Import ({0})", "Nhập ({0})";
    MentionLibrary => "Library", "Thư viện";
    JuryInterrupt => "Interrupt", "Ngắt";
    JuryInterrupting => "Interrupting…", "Đang ngắt…";
    JuryReplay => "Replay", "Xem lại";
    JuryReplayExit => "Exit replay", "Thoát xem lại";
    JuryReadOnly => "Read-only", "Chỉ xem";
    JuryReplaySpeed => "Replay speed", "Tốc độ xem lại";
    JuryPaused => "Paused", "Tạm dừng";
    JuryFast => "Fast", "Nhanh";
    JuryInstant => "Instant", "Tức thì";
    MediaBatchImage => "Generating illustrations", "Đang tạo hình minh hoạ";
    MediaBatchAudio => "Generating audio", "Đang tạo âm thanh";
    MediaBatchVideo => "Generating video", "Đang tạo video";
    MediaCountImage => "{0} images", "{0} ảnh";
    MediaCountAudio => "{0} clips", "{0} đoạn âm thanh";
    MediaCountVideo => "{0} videos", "{0} video";
    MediaOpenCell => "Open item {0}", "Mở mục {0}";
    MediaFailed => "Failed", "Lỗi";
    MediaRetryMessage => "{0} of the {1} failed to generate, please try those again ({2}).", "{0} trong {1} tạo không được, hãy tạo lại các mục đó ({2}).";
    BrandStepTitle => "Your brand", "Thương hiệu";
    BrandStepBody => "Give the website of your brand and the agent extracts a design system from it: palette, type, components. New projects then start with it.", "Nhập website thương hiệu, agent sẽ trích design system từ đó: bảng màu, kiểu chữ, component. Project mới sẽ dùng nó từ đầu.";
    BrandStepUrl => "Brand website", "Website thương hiệu";
    BrandStepExtract => "Extract design system", "Trích design system";
    BrandStepDone => "{0} is ready and set as the default design system.", "{0} đã sẵn sàng và được đặt làm design system mặc định.";
    BrandStepWait => "Wait until the design system is extracted", "Chờ trích xong design system";
    Optional => "Optional", "Tuỳ chọn";
    HomeTabsLabel => "Start from", "Bắt đầu từ";
    HomeTabDesigns => "Designs", "Thiết kế";
    HomeTabTemplates => "Templates", "Template";
    HomeTabSystems => "Design systems", "Design system";
    HomeTabImage => "Image templates", "Template ảnh";
    HomeTabVideo => "Video templates", "Template video";
    HomeNoDesigns => "No designs yet. Describe one above.", "Chưa có thiết kế nào. Mô tả một cái ở trên.";
    HomeNoSystems => "No design systems yet.", "Chưa có design system nào.";
    HomeAllDesigns => "All projects", "Mọi project";
    QuickSettings => "Quick settings", "Cài đặt nhanh";
    LocalMode => "Local mode", "Chế độ cục bộ";
    LocalModeHint => "No account. Everything stays on this computer.", "Không cần tài khoản. Mọi thứ nằm trên máy này.";
    Help => "Help", "Trợ giúp";
    Documentation => "Documentation", "Tài liệu hướng dẫn";
    ReportIssue => "Report issue", "Báo lỗi";
    ReportCopied => "Diagnostics copied to the clipboard.", "Đã chép thông tin chẩn đoán vào clipboard.";
    ReportCopiedDir => "Diagnostics copied to the clipboard. Logs: {0}", "Đã chép thông tin chẩn đoán vào clipboard. Log: {0}";
    ReportManual => "The clipboard is not available here. Select the text, copy it and paste it where you report the issue. The log folder is open.", "Không dùng được clipboard ở đây. Chọn đoạn chữ, chép và dán vào nơi báo lỗi. Thư mục log đã được mở.";
    ExportJsx => "Export as JSX", "Export dạng JSX";
    ExportJsxHtml => "Export React component as HTML", "Xuất component React thành HTML";
    FontsMissingTitle => "Brand font files missing", "Thiếu file font thương hiệu";
    FontsMissingBody => "Typography previews use substitute fonts until brand font files are added.", "Phần xem trước kiểu chữ đang dùng font thay thế cho tới khi thêm file font thương hiệu.";
    FontsMissingAdd => "Add brand font files", "Thêm file font thương hiệu";
    FontsMissingKeep => "Keep substitutes", "Giữ font thay thế";
    // End of track C2-chat-home.

    // Track C4-depth: Crystallize and evolution proposals (Automations).
    Crystallize => "Crystallize", "Đúc kết";
    Crystallizing => "Crystallizing…", "Đang đúc kết…";
    CrystallizeTitle => "Draft skill and memory proposals from this run", "Soạn đề xuất skill và memory từ lượt chạy này";
    CrystallizeFailed => "Crystallize failed: {0}", "Đúc kết không được: {0}";
    CrystallizeNoProposals => "Crystallize finished, but no proposals were returned.", "Đã đúc kết nhưng không có đề xuất nào.";
    ProposalsTitle => "Evolution proposals", "Đề xuất tiến hoá";
    ProposalsSub => "Review automation output before it changes memory, skills or design systems.", "Duyệt kết quả của automation trước khi nó đổi memory, skill hay design system.";
    ProposalsPending => "{0} pending", "{0} chờ duyệt";
    ProposalApply => "Apply", "Áp dụng";
    ProposalReject => "Reject", "Bỏ";
    ProposalsDismissReason => "Dismissed in Automations", "Bỏ ở trang Automations";
    ProposalTargetMemory => "Memory", "Memory";
    ProposalTargetSkill => "Skill", "Skill";
    ProposalActionCreate => "Create", "Tạo mới";
    ProposalPreview => "Preview", "Xem trước";
    ProposalSkillApplied => "Saved as skill {0}.", "Đã lưu thành skill {0}.";
    ProposalMemoryApplied => "Saved to memory.", "Đã lưu vào memory.";
    // Track C4-depth: GenUI surfaces in the chat.
    GenuiWaiting => "A plugin is waiting for an answer", "Plugin đang chờ trả lời";
    GenuiContinue => "Continue", "Tiếp tục";
    GenuiAuthorize => "Authorize", "Cấp quyền";
    GenuiSkip => "Skip", "Bỏ qua";
    GenuiAcceptAll => "Accept all", "Nhận tất cả";
    GenuiRejectAll => "Reject all", "Bỏ tất cả";
    GenuiPartial => "Pick per file", "Chọn từng file";
    GenuiAccept => "Accept", "Nhận";
    GenuiReject => "Reject", "Bỏ";
    GenuiSubmit => "Submit", "Gửi";
    GenuiSubmitPartial => "Submit partial decision", "Gửi lựa chọn từng file";
    GenuiPatchNotes => "Notes for the patch author (optional)", "Ghi chú cho người viết patch (không bắt buộc)";
    GenuiConfirmDefault => "The plugin needs your confirmation to continue.", "Plugin cần xác nhận để chạy tiếp.";
    GenuiAuthorizeDefault => "Authorize {0} to continue.", "Cấp quyền cho {0} để chạy tiếp.";
    GenuiTheConnection => "the connection", "kết nối";
    GenuiConnectorHint => "Connector: {0}", "Connector: {0}";
    GenuiMcpHint => "MCP server: {0}", "MCP server: {0}";
    GenuiReviewDefault => "Review the diff and choose how to proceed.", "Xem diff rồi chọn cách xử lý.";
    GenuiFilesTouched => "{0} files touched.", "{0} file đã sửa.";
    GenuiNoFileList => "No file list yet: the decision covers every file.", "Chưa có danh sách file: lựa chọn áp cho mọi file.";
    GenuiPickEach => "Pick accept or reject for: {0}", "Chọn nhận hoặc bỏ cho: {0}";
    GenuiAnswerDefault => "The plugin needs an answer to continue.", "Plugin cần câu trả lời để chạy tiếp.";
    GenuiJsonPlaceholder => "Answer as JSON or plain text", "Trả lời bằng JSON hoặc chữ thường";
    GenuiFieldRequired => "{0} is required.", "Cần điền {0}.";
    GenuiFieldNumber => "{0} must be a number.", "{0} phải là số.";
    GenuiFieldWhole => "{0} must be a whole number.", "{0} phải là số nguyên.";
    GenuiFieldRange => "{0} is out of range.", "{0} nằm ngoài khoảng cho phép.";
    GenuiAnswerFailed => "Could not send the answer: {0}", "Không gửi được câu trả lời: {0}";
    GenuiRemembered => "Remembered answers ({0})", "Câu trả lời đã nhớ ({0})";
    GenuiRememberedHint => "Plugins reuse these answers without asking again.", "Plugin dùng lại các câu trả lời này mà không hỏi lại.";
    GenuiRevoke => "Revoke", "Thu hồi";
    GenuiTierProject => "project", "project";
    GenuiTierConversation => "conversation", "cuộc trò chuyện";
}

/// The current language's text for `key`.
#[must_use]
pub fn t(key: T) -> &'static str {
    key.text(locale())
}

/// Content text (template and skill descriptions, copied from the upstream
/// app) with the upstream product name swapped for Contour. The files keep
/// their original wording and attribution; only the UI shows the new name.
#[must_use]
pub fn rebrand(text: &str) -> std::borrow::Cow<'_, str> {
    if names_original_app(text) {
        text.replace("OpenDesign", "Contour")
            .replace("Open Design", "Contour")
            .replace("Open-Design", "Contour")
            .into()
    } else {
        text.into()
    }
}

/// Whether `text` names the original app (`docs/brand.md`: never in the
/// UI). The lowercase `open-design` profile and manifest names are
/// identifiers, not the name.
#[must_use]
pub fn names_original_app(text: &str) -> bool {
    ["OpenDesign", "Open Design", "Open-Design"]
        .iter()
        .any(|n| text.contains(n))
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
            "Create refreshable Contour artifacts, made with Contour."
        );
        assert!(matches!(
            rebrand("A tea shop"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    use super::*;

    /// A name the UI must not show: the upstream app's, or Contour's
    /// previous name "Nét" (composed or decomposed accent).
    fn retired_name(text: &str) -> bool {
        names_original_app(text) || ["N\u{e9}t", "Ne\u{301}t"].iter().any(|n| text.contains(n))
    }

    #[test]
    fn the_retired_names_are_caught() {
        for text in [
            "Open Design",
            "OpenDesign",
            "Open-Design",
            "Mở Nét…",
            "Ne\u{301}t projects",
        ] {
            assert!(retired_name(text), "{text}");
        }
        for text in ["Contour projects", "nét vẽ", "the open-design profile"] {
            assert!(!retired_name(text), "{text}");
        }
    }

    #[test]
    fn the_in_app_guide_uses_the_current_name() {
        for (tag, guide) in [
            ("en", include_str!("../docs/guide.en.md")),
            ("vi", include_str!("../docs/guide.vi.md")),
        ] {
            // The dsh profile is called `open-design`; only names count.
            let hits: Vec<&str> = guide.lines().filter(|l| retired_name(l)).collect();
            assert!(
                hits.is_empty(),
                "guide.{tag}.md shows a retired name: {hits:?}"
            );
        }
    }

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
            assert!(
                !retired_name(en) && !retired_name(vi),
                "{key:?} shows a retired name"
            );
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

    #[test]
    fn locale_parse_follows_the_original_resolver() {
        for (tag, want) in [
            ("de-AT", Locale::De),
            ("pt", Locale::PtBr),
            ("pt-PT", Locale::PtBr),
            ("es-MX", Locale::EsEs),
            ("zh", Locale::ZhCn),
            ("zh-Hans-CN", Locale::ZhCn),
            ("zh-Hant", Locale::ZhTw),
            ("zh-HK", Locale::ZhTw),
            ("zh_TW", Locale::ZhTw),
            ("ZH-cn", Locale::ZhCn),
            ("fa-IR", Locale::Fa),
            ("ja-JP", Locale::Ja),
            ("xx", Locale::En),
        ] {
            assert_eq!(Locale::parse(tag), want, "{tag}");
        }
        for l in Locale::ALL {
            assert_eq!(Locale::parse(l.tag()), *l, "round trip {l:?}");
        }
        assert_eq!(Locale::ALL.len(), 20, "en, vi and the original's 18 others");
        assert!(Locale::Ar.is_rtl() && Locale::Fa.is_rtl() && !Locale::De.is_rtl());
    }

    #[test]
    fn a_loaded_language_falls_back_to_english_per_key() {
        assert!(!has_dict(Locale::Hu));
        assert_eq!(T::NavHome.text(Locale::Hu), "Home", "not loaded yet");
        let unknown = install_dict(
            Locale::Hu,
            "# comment\n\nNavHome = Kezdőlap\nNoSuchKey = x\nnot a line\nPasteDesignMdPlaceholder = a\\nb\n".to_owned(),
        );
        assert!(has_dict(Locale::Hu));
        assert_eq!(unknown, ["NoSuchKey", "not a line"]);
        assert_eq!(T::NavHome.text(Locale::Hu), "Kezdőlap");
        assert_eq!(T::NavSettings.text(Locale::Hu), "Settings", "missing key");
        assert_eq!(T::PasteDesignMdPlaceholder.text(Locale::Hu), "a\nb");
        set_locale(Locale::Hu);
        assert_eq!(t(T::NavHome), "Kezdőlap");
        set_locale(Locale::En);
    }

    /// Display width: East Asian wide characters count twice.
    fn width(s: &str) -> usize {
        s.chars()
            .map(|c| match c as u32 {
                0x1100..=0x115F
                | 0x2E80..=0xA4CF
                | 0xAC00..=0xD7A3
                | 0xF900..=0xFAFF
                | 0xFE30..=0xFE4F
                | 0xFF00..=0xFF60
                | 0xFFE0..=0xFFE6 => 2,
                _ => 1,
            })
            .sum()
    }

    /// Every language file: present, every key translated once, same
    /// placeholders as English, no em dash, and no string so much longer
    /// than English that it breaks the layout built around it.
    #[test]
    fn every_language_file_is_complete_and_fits() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let mut failures = Vec::new();
        for &locale in Locale::ALL.iter().filter(|l| l.is_loaded_on_demand()) {
            let path = dir.join(format!("{}.txt", locale.tag()));
            let Ok(text) = std::fs::read_to_string(&path) else {
                failures.push(format!("{}: missing", path.display()));
                continue;
            };
            let text: &'static str = Box::leak(text.into_boxed_str());
            let (slots, unknown) = parse_dict(text);
            let tag = locale.tag();
            for u in unknown {
                failures.push(format!("{tag}: unknown key or bad line {u:?}"));
            }
            let mut seen = std::collections::HashSet::new();
            for line in text
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            {
                if let Some((k, _)) = line.split_once(" = ") {
                    if !seen.insert(k.trim()) {
                        failures.push(format!("{tag}: {k} twice"));
                    }
                }
            }
            for (key, slot) in T::ALL.iter().zip(&slots) {
                let en = key.text(Locale::En);
                let Some(text) = slot else {
                    failures.push(format!("{tag}: {} untranslated", key.name()));
                    continue;
                };
                if text.trim().is_empty() || placeholders(text) != placeholders(en) {
                    failures.push(format!("{tag}: {} placeholders: {text}", key.name()));
                }
                if text.contains('\u{2014}') {
                    failures.push(format!("{tag}: {} has an em dash", key.name()));
                }
                if retired_name(text) {
                    failures.push(format!("{tag}: {} shows a retired name", key.name()));
                }
                let (we, wt) = (width(en), width(text));
                if wt > (2 * we).max(we + 12) {
                    failures.push(format!(
                        "{tag}: {} too long ({wt} vs {we}): {text}",
                        key.name()
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} problems:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    /// Settings ▸ About checks for updates, and its subtitle says so in
    /// every language, not only in English (beta.7).
    #[test]
    fn the_about_subtitle_mentions_updates_in_every_language() {
        const UPDATES: &[(&str, &str)] = &[
            ("ar", "التحديثات"),
            ("de", "Updates"),
            ("es-ES", "actualizaciones"),
            ("fa", "به‌روزرسانی"),
            ("fr", "mises à jour"),
            ("hu", "frissítések"),
            ("id", "pembaruan"),
            ("it", "aggiornamenti"),
            ("ja", "アップデート"),
            ("ko", "업데이트"),
            ("pl", "aktualizacje"),
            ("pt-BR", "atualizações"),
            ("ru", "обновления"),
            ("th", "การอัปเดต"),
            ("tr", "güncellemeler"),
            ("uk", "оновлення"),
            ("zh-CN", "更新"),
            ("zh-TW", "更新"),
        ];
        assert!(T::AboutHint.text(Locale::En).contains("updates"));
        assert!(T::AboutHint.text(Locale::Vi).contains("cập nhật"));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let on_demand = Locale::ALL.iter().filter(|l| l.is_loaded_on_demand());
        assert_eq!(
            on_demand.count(),
            UPDATES.len(),
            "a language without a word here"
        );
        for (tag, word) in UPDATES {
            let text = std::fs::read_to_string(dir.join(format!("{tag}.txt"))).unwrap();
            let hint = text
                .lines()
                .find_map(|l| l.strip_prefix("AboutHint = "))
                .unwrap_or_default();
            assert!(hint.contains(word), "{tag}: {hint}");
        }
    }
}
