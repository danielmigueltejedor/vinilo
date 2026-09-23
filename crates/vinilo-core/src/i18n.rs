// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-FileCopyrightText: 2026 Miguel Rincon
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime language for both clients.
//!
//! English and Spanish are selected in the app, not taken from `LANG`. A first
//! run has no file yet; that is how the GNOME client knows to open the picker
//! before Apple's sign-in. The choice lives in `~/.config/vinilo/locale` so
//! Vinilo and Aguja stay in agreement without sharing a GTK settings parser.

use std::cell::Cell;
use std::path::PathBuf;

use crate::paths;

/// The two languages Vinilo ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "en" | "english" => Some(Self::English),
            "es" | "spanish" | "español" | "espanol" => Some(Self::Spanish),
            _ => None,
        }
    }

    /// Index in the Preferences combo, and back.
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::Spanish,
            _ => Self::English,
        }
    }

    pub fn index(self) -> u32 {
        match self {
            Self::English => 0,
            Self::Spanish => 1,
        }
    }

    /// Always in the language itself, so the combo is readable before you pick.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
        }
    }
}

thread_local! {
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
}

/// The language UI strings currently resolve to.
pub fn current() -> Language {
    CURRENT.with(Cell::get)
}

/// Point every later `t()` call at this language. GTK-thread only, like gettext.
pub fn set_current(language: Language) {
    CURRENT.with(|cell| cell.set(language));
}

fn locale_path() -> Option<PathBuf> {
    Some(paths::config_dir()?.join("locale"))
}

/// `None` on a first run, or if the file is missing or unreadable.
pub fn load() -> Option<Language> {
    let text = std::fs::read_to_string(locale_path()?).ok()?;
    Language::parse(&text)
}

/// Persist the choice. Best-effort: failing to save must never block playback.
pub fn save(language: Language) {
    let Some(path) = locale_path() else {
        return;
    };
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        tracing::warn!(?err, "could not create config directory");
        return;
    }
    if let Err(err) = std::fs::write(&path, format!("{}\n", language.as_str())) {
        tracing::warn!(?err, "could not save language");
    }
}

/// Every user-visible sentence the clients share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    AppName,
    Search,
    Songs,
    Albums,
    Artists,
    Playlists,
    Discover,
    RecentlyPlayed,
    MadeForYou,
    RecommendedSongs,
    RecentlyAdded,
    Charts,
    LoadingDiscover,
    DiscoverEmpty,
    DiscoverEmptyBody,
    Library,
    All,
    Queue,
    AppleMusic,
    MainMenu,
    ToggleSidebar,
    ShowSidebar,
    HideSidebar,
    Reload,
    Sort,
    SearchFilter,
    Play,
    Pause,
    Shuffle,
    Repeat,
    RepeatOff,
    RepeatAll,
    RepeatOne,
    Previous,
    Next,
    Volume,
    ClearQueue,
    NothingQueued,
    NothingQueuedBody,
    RemoveFromQueue,
    TrackOptions,
    PlayNext,
    AddToQueue,
    AddToLibrary,
    RemoveFromLibrary,
    Favourite,
    RemoveFavourite,
    GoToAlbum,
    GoToArtist,
    Preferences,
    KeyboardShortcuts,
    About,
    SignOut,
    Quit,
    Appearance,
    Theme,
    ThemeSystem,
    ThemeLight,
    ThemeDark,
    AccentColour,
    AccentAppleRed,
    AccentSystem,
    AccentBlue,
    AccentPurple,
    AccentGreen,
    AccentOrange,
    AlbumArtBackdrop,
    AlbumArtBackdropSub,
    SourceTint,
    SourceTintSub,
    Language,
    LanguageSub,
    MusicSource,
    MusicSourceSub,
    ProviderTitle,
    ProviderBody,
    ProviderApple,
    ProviderAppleSub,
    ProviderLocal,
    ProviderLocalSub,
    ProviderSpotify,
    ProviderSpotifySub,
    ProviderYoutube,
    ProviderYoutubeSub,
    ProviderTidal,
    ProviderTidalSub,
    ProviderNote,
    ProviderRestart,
    CatalogSetupBody,
    CatalogSetupYtOk,
    CatalogSetupYtMissing,
    CatalogSetupContinue,
    CatalogSetupBack,
    CatalogSignInNote,
    CatalogLoginDone,
    CatalogOpenBrowser,
    CatalogSignOutTitle,
    CatalogSignOutBody,
    SearchSpotifyBody,
    SearchYoutubeBody,
    SearchTidalBody,
    EmptyLibraryBodyLocal,
    EmptyLibraryBodyCatalog,
    SearchSpotify,
    SearchYoutube,
    SearchTidal,
    Notifications,
    NotifyTrackChange,
    NotifyTrackChangeSub,
    WelcomeTitle,
    WelcomeBody,
    SignIn,
    SignInNote,
    SignOutTitle,
    SignOutBody,
    Cancel,
    StartingEngine,
    Connecting,
    PlaybackUnavailable,
    Ready,
    NothingPlaying,
    SearchAppleMusic,
    SearchAppleMusicBody,
    EmptyLibrary,
    EmptyLibraryBody,
    NoMatches,
    LoadingLibrary,
    LoadingAlbums,
    LoadingArtists,
    LoadingPlaylists,
    SearchingCatalog,
    SearchLibrary,
    SearchAlbums,
    SearchArtists,
    SearchPlaylists,
    SortTitle,
    SortArtist,
    SortAlbum,
    SortYear,
    SortAdded,
    SortUpdated,
    ReverseOrder,
    FilterEverything,
    CouldNotLoadPage,
    NothingHereYet,
    PlayPage,
    PinPlaylists,
    PinAPlaylist,
    PinAll,
    UnpinAll,
    AddToSidebar,
    RemoveFromSidebar,
    Unavailable,
    NoPlaylistsYet,
    ShortcutsPlayback,
    ShortcutsGeneral,
    ShortcutPlayPause,
    ShortcutNext,
    ShortcutPrevious,
    ShortcutVolumeUp,
    ShortcutVolumeDown,
    ShortcutSearch,
    ShortcutCloseWindow,
    ShortcutToggleSidebar,
    ShortcutToggleQueue,
    ShortcutPreferences,
    ShortcutShortcuts,
    ShortcutQuit,
    EarlierOne,
    HideEarlier,
    ShowEarlier,
    TracksOne,
    Album,
    Artist,
    Playlist,
    Song,
    SongsLower,
    EmptyAlbum,
    EmptyArtist,
    EmptyPlaylist,
    AboutComments,
    RefreshingLibrary,
    ToastAddLibrary,
    ToastFavourite,
    ToastRemoveLibrary,
    ToastRemoveFavourite,
    ToastGoneFromQueue,
    ToastUnstreamable,
    LikedSongs,
    YourTopTracks,
    SpotifyNotSignedIn,
    SpotifyTokenRefused,
    CatalogLoginFailed,
    CatalogLibraryFailed,
    SpotifyRateLimited,
    SpotifyTimeout,
    SpotifyCatalogueChanged,
    NewPlaylist,
    NewPlaylistTitle,
    NewPlaylistPlaceholder,
    Create,
    AddToPlaylist,
    ToastCreatePlaylist,
    ToastAddToPlaylist,
    WriteWrongSource,
    Lyrics,
    LyricsLoading,
    LyricsMissing,
}

/// Resolve `key` in the current language.
pub fn t(key: Key) -> &'static str {
    match current() {
        Language::English => en(key),
        Language::Spanish => es(key),
    }
}

pub fn earlier(n: usize) -> String {
    if n == 1 {
        t(Key::EarlierOne).into()
    } else {
        earlier_n(n)
    }
}

pub fn tracks(n: usize) -> String {
    if n == 1 {
        t(Key::TracksOne).into()
    } else {
        tracks_n(n)
    }
}

pub fn songs_count(n: usize) -> String {
    if n == 1 {
        format!("1 {}", t(Key::Song))
    } else {
        format!("{n} {}", t(Key::SongsLower))
    }
}

fn en(key: Key) -> &'static str {
    match key {
        Key::AppName => "Vinilo",
        Key::Search => "Search",
        Key::Songs => "Songs",
        Key::Albums => "Albums",
        Key::Artists => "Artists",
        Key::Playlists => "Playlists",
        Key::Discover => "Listen Now",
        Key::RecentlyPlayed => "Recently Played",
        Key::MadeForYou => "Made for You",
        Key::RecommendedSongs => "Songs for You",
        Key::RecentlyAdded => "Recently Added",
        Key::Charts => "Charts",
        Key::LoadingDiscover => "Loading recommendations",
        Key::DiscoverEmpty => "Nothing to discover yet",
        Key::DiscoverEmptyBody => {
            "Play something and Vinilo will show it here, along with playlists \
             and songs this source recommends."
        }
        Key::Library => "Library",
        Key::All => "All",
        Key::Queue => "Queue",
        Key::AppleMusic => "Apple Music",
        Key::MainMenu => "Main Menu",
        Key::ToggleSidebar => "Toggle Sidebar",
        Key::ShowSidebar => "Show sidebar",
        Key::HideSidebar => "Hide sidebar",
        Key::Reload => "Reload",
        Key::Sort => "Sort",
        Key::SearchFilter => "What to search for",
        Key::Play => "Play",
        Key::Pause => "Pause",
        Key::Shuffle => "Shuffle",
        Key::Repeat => "Repeat",
        Key::RepeatOff => "Repeat: off",
        Key::RepeatAll => "Repeat: all",
        Key::RepeatOne => "Repeat: this track",
        Key::Previous => "Previous",
        Key::Next => "Next",
        Key::Volume => "Volume",
        Key::ClearQueue => "Clear the queue",
        Key::NothingQueued => "Nothing queued",
        Key::NothingQueuedBody => "Play something and it will show up here.",
        Key::RemoveFromQueue => "Remove from queue",
        Key::TrackOptions => "Track options",
        Key::PlayNext => "Play _Next",
        Key::AddToQueue => "Add to _Queue",
        Key::AddToLibrary => "Add to _Library",
        Key::RemoveFromLibrary => "_Remove from Library",
        Key::Favourite => "_Favourite",
        Key::RemoveFavourite => "Remove _Favourite",
        Key::GoToAlbum => "Go to _Album",
        Key::GoToArtist => "Go to A_rtist",
        Key::Preferences => "_Preferences",
        Key::KeyboardShortcuts => "_Keyboard Shortcuts",
        Key::About => "_About Vinilo",
        Key::SignOut => "_Sign Out",
        Key::Quit => "_Quit",
        Key::Appearance => "Appearance",
        Key::Theme => "Theme",
        Key::ThemeSystem => "Follow System",
        Key::ThemeLight => "Light",
        Key::ThemeDark => "Dark",
        Key::AccentColour => "Accent Colour",
        Key::AccentAppleRed => "Apple Music Red",
        Key::AccentSystem => "Follow System",
        Key::AccentBlue => "Blue",
        Key::AccentPurple => "Purple",
        Key::AccentGreen => "Green",
        Key::AccentOrange => "Orange",
        Key::AlbumArtBackdrop => "Colour from the cover",
        Key::AlbumArtBackdropSub => "A wash of the album's main colour behind the player",
        Key::SourceTint => "Tint from music source",
        Key::SourceTintSub => "Apple Music red, Spotify green, YouTube red, Tidal cyan",
        Key::Language => "Language",
        Key::LanguageSub => "Interface language. Takes effect immediately.",
        Key::MusicSource => "Music source",
        Key::MusicSourceSub => {
            "Where Vinilo searches and plays. Changing source restarts the player."
        }
        Key::ProviderTitle => "Where is your music?",
        Key::ProviderBody => {
            "Pick a catalogue. Apple Music is stable; Spotify and YouTube Music are Beta: they work, the Apple sidecar is still heavy. Tidal is Alpha. Files on this computer always play."
        }
        Key::ProviderApple => "Apple Music",
        Key::ProviderAppleSub => {
            "Your library and the catalogue. Works well; the hidden player is still a full Chromium."
        }
        Key::ProviderLocal => "This computer",
        Key::ProviderLocalSub => "Files you open, and folders you drop on Vinilo",
        Key::ProviderSpotify => "Spotify (Beta)",
        Key::ProviderSpotifySub => {
            "Library and search after sign-in. Native audio needs Spotify Premium."
        }
        Key::ProviderYoutube => "YouTube Music (Beta)",
        Key::ProviderYoutubeSub => {
            "Library and search after sign-in. Audio comes from YouTube Music itself."
        }
        Key::ProviderTidal => "Tidal (Alpha)",
        Key::ProviderTidalSub => "Search after you sign in. Playback still goes through yt-dlp.",
        Key::ProviderNote => {
            "You can change this later in Preferences. Beta sources work; Alpha is usable, not finished. Spotify Premium plays through librespot; without it, yt-dlp is the fallback."
        }
        Key::ProviderRestart => "Could not switch source while another Vinilo window is open.",
        Key::CatalogSetupBody => {
            "Sign in so Vinilo can use your session with this catalogue. A window opens, the same way Apple Music does."
        }
        Key::CatalogSetupYtOk => {
            "After you sign in, search a song and click it. YouTube Music prefers InnerTube audio; Tidal still needs yt-dlp."
        }
        Key::CatalogSetupYtMissing => {
            "yt-dlp is not on PATH. YouTube Music still plays from InnerTube when it can. Tidal needs yt-dlp (sudo pacman -S yt-dlp ffmpeg)."
        }
        Key::CatalogSetupContinue => "Sign In",
        Key::CatalogSetupBack => "Choose another source",
        Key::CatalogSignInNote => {
            "The sign-in page opens in a separate window, including two-factor if your account uses it. It closes once you are in."
        }
        Key::CatalogLoginDone => "Done",
        Key::CatalogOpenBrowser => "Open in browser",
        Key::CatalogSignOutTitle => "Sign out of this catalogue?",
        Key::CatalogSignOutBody => {
            "Vinilo will forget this session. Signing back in opens the login window again."
        }
        Key::SearchSpotifyBody => "Find a Spotify track and click it to play.",
        Key::SearchYoutubeBody => "Find a YouTube Music track and click it to play.",
        Key::SearchTidalBody => "Find a Tidal track and click it to play.",
        Key::EmptyLibraryBodyLocal => {
            "Open an audio file with Vinilo, drop a folder on the window, or set Vinilo as the default player for music in GNOME Settings."
        }
        Key::EmptyLibraryBodyCatalog => {
            "Liked songs, albums and playlists from this account will appear here. \
             If this stays empty, sign in again from the menu."
        }
        Key::Notifications => "Notifications",
        Key::NotifyTrackChange => "Notify on track change",
        Key::NotifyTrackChangeSub => "When a new song starts and Vinilo is not in focus",
        Key::WelcomeTitle => "Welcome to Vinilo",
        Key::WelcomeBody => {
            "Vinilo plays your Apple Music library natively on GNOME. \
             It needs an active Apple Music subscription."
        }
        Key::SignIn => "Sign In to Apple Music",
        Key::SignInNote => {
            "Apple's own sign-in page opens in a separate window, including \
             two-factor if your account uses it. It closes for good once you're in."
        }
        Key::SignOutTitle => "Sign out of Apple Music?",
        Key::SignOutBody => {
            "Vinilo will forget this session. Signing back in opens Apple's \
             login window again."
        }
        Key::Cancel => "Cancel",
        Key::StartingEngine => "Starting the playback engine",
        Key::Connecting => "Connecting to Apple Music",
        Key::PlaybackUnavailable => "Playback unavailable",
        Key::Ready => "Ready",
        Key::NothingPlaying => "Nothing playing",
        Key::SearchAppleMusic => "Search Apple Music",
        Key::SearchAppleMusicBody => "Find songs from the whole catalogue, not just your library.",
        Key::SearchSpotify => "Search Spotify",
        Key::SearchYoutube => "Search YouTube Music",
        Key::SearchTidal => "Search Tidal",
        Key::EmptyLibrary => "Nothing here yet",
        Key::EmptyLibraryBody => {
            "Vinilo refreshes your library after sign-in. Use Reload to try again. You can also open a file on this computer — Vinilo plays those too."
        }
        Key::NoMatches => "No matches",
        Key::LoadingLibrary => "Loading your library",
        Key::LoadingAlbums => "Loading your albums",
        Key::LoadingArtists => "Loading your artists",
        Key::LoadingPlaylists => "Loading your playlists",
        Key::SearchingCatalog => "Searching Apple Music",
        Key::SearchLibrary => "Search your library",
        Key::SearchAlbums => "Search albums",
        Key::SearchArtists => "Search artists",
        Key::SearchPlaylists => "Search playlists",
        Key::SortTitle => "Title",
        Key::SortArtist => "Artist",
        Key::SortAlbum => "Album",
        Key::SortYear => "Year",
        Key::SortAdded => "Recently Added",
        Key::SortUpdated => "Recently Updated",
        Key::ReverseOrder => "_Reverse Order",
        Key::FilterEverything => "Everything",
        Key::CouldNotLoadPage => "Could not load this page",
        Key::NothingHereYet => "Nothing here yet",
        Key::PlayPage => "Play",
        Key::PinPlaylists => "Pin Playlists",
        Key::PinAPlaylist => "Pin a playlist",
        Key::PinAll => "Pin All",
        Key::UnpinAll => "Unpin All",
        Key::AddToSidebar => "_Add to the Sidebar",
        Key::RemoveFromSidebar => "_Remove from the Sidebar",
        Key::Unavailable => "Unavailable",
        Key::NoPlaylistsYet => "Your library has no playlists yet, or it has not finished loading.",
        Key::ShortcutsPlayback => "Playback",
        Key::ShortcutsGeneral => "General",
        Key::ShortcutPlayPause => "Play or pause",
        Key::ShortcutNext => "Next track",
        Key::ShortcutPrevious => "Previous track",
        Key::ShortcutVolumeUp => "Volume up",
        Key::ShortcutVolumeDown => "Volume down",
        Key::ShortcutSearch => "Search",
        Key::ShortcutCloseWindow => "Close the window",
        Key::ShortcutToggleSidebar => "Toggle the sidebar",
        Key::ShortcutToggleQueue => "Toggle the queue",
        Key::ShortcutPreferences => "Preferences",
        Key::ShortcutShortcuts => "Keyboard shortcuts",
        Key::ShortcutQuit => "Quit",
        Key::EarlierOne => "1 earlier",
        Key::HideEarlier => "Hide the tracks before this one",
        Key::ShowEarlier => "Show the tracks before this one",
        Key::TracksOne => "1 track",
        Key::Album => "album",
        Key::Artist => "artist",
        Key::Playlist => "playlist",
        Key::Song => "song",
        Key::SongsLower => "songs",
        Key::EmptyAlbum => "This album has no songs.",
        Key::EmptyArtist => "This artist has no albums.",
        Key::EmptyPlaylist => "This playlist has no songs.",
        Key::AboutComments => {
            "A native GNOME music player for Linux.\n\n\
             Apple Music plays through Apple's MusicKit player using \
             Google's Widevine CDM, in a hidden helper process. Files on this \
             computer play natively. Spotify (Beta) plays through librespot \
             when you have Premium; YouTube Music (Beta) prefers InnerTube \
             audio; Tidal (Alpha) still uses yt-dlp. Each catalogue keeps a \
             cookie sign-in for library and search.\n\n\
             Fork of Slipmat by Miguel Rincon."
        }
        Key::RefreshingLibrary => "Refreshing library…",
        Key::ToastAddLibrary => "Adding to your library…",
        Key::ToastFavourite => "Favouriting…",
        Key::ToastRemoveLibrary => "Removing from your library…",
        Key::ToastRemoveFavourite => "Removing favourite…",
        Key::ToastGoneFromQueue => "That track is no longer in the queue",
        Key::ToastUnstreamable => "Nothing here can be streamed",
        Key::LikedSongs => "Liked Songs",
        Key::YourTopTracks => "Your Top Tracks",
        Key::SpotifyNotSignedIn => {
            "Spotify is not signed in. Open Sign In from the menu and log in again."
        }
        Key::SpotifyTokenRefused => {
            "Spotify would not issue a token. Sign in again from the menu if this keeps happening."
        }
        Key::CatalogLoginFailed => {
            "Could not keep the session. Stay on the signed-in page, then press Done."
        }
        Key::CatalogLibraryFailed => {
            "Could not load this catalog's library. Try Refresh, or sign in again if this source needs it."
        }
        Key::SpotifyRateLimited => "Spotify asked us to wait. Try Reload in a minute.",
        Key::SpotifyTimeout => "Spotify did not answer in time. Try again.",
        Key::SpotifyCatalogueChanged => {
            "Spotify's catalogue API changed. Sign in again, then Reload."
        }
        Key::NewPlaylist => "New _playlist",
        Key::NewPlaylistTitle => "New playlist",
        Key::NewPlaylistPlaceholder => "Playlist name",
        Key::Create => "Create",
        Key::AddToPlaylist => "Add to _playlist",
        Key::ToastCreatePlaylist => "Creating playlist…",
        Key::ToastAddToPlaylist => "Adding to playlist…",
        Key::WriteWrongSource => "That item belongs to another music source.",
        Key::Lyrics => "Lyrics",
        Key::LyricsLoading => "Loading lyrics…",
        Key::LyricsMissing => "No lyrics for this song",
    }
}

fn es(key: Key) -> &'static str {
    match key {
        Key::AppName => "Vinilo",
        Key::Search => "Buscar",
        Key::Songs => "Canciones",
        Key::Albums => "Álbumes",
        Key::Artists => "Artistas",
        Key::Playlists => "Listas",
        Key::Discover => "Escuchar ahora",
        Key::RecentlyPlayed => "Reproducido recientemente",
        Key::MadeForYou => "Hecho para ti",
        Key::RecommendedSongs => "Canciones para ti",
        Key::RecentlyAdded => "Añadido recientemente",
        Key::Charts => "Éxitos",
        Key::LoadingDiscover => "Cargando recomendaciones",
        Key::DiscoverEmpty => "Todavía no hay nada que descubrir",
        Key::DiscoverEmptyBody => {
            "Reproduce algo y Vinilo lo mostrará aquí, junto con listas y \
             canciones que recomienda esta fuente."
        }
        Key::Library => "Biblioteca",
        Key::All => "Todas",
        Key::Queue => "Cola",
        Key::AppleMusic => "Apple Music",
        Key::MainMenu => "Menú principal",
        Key::ToggleSidebar => "Mostrar u ocultar la barra lateral",
        Key::ShowSidebar => "Mostrar la barra lateral",
        Key::HideSidebar => "Ocultar la barra lateral",
        Key::Reload => "Recargar",
        Key::Sort => "Ordenar",
        Key::SearchFilter => "Qué buscar",
        Key::Play => "Reproducir",
        Key::Pause => "Pausar",
        Key::Shuffle => "Aleatorio",
        Key::Repeat => "Repetir",
        Key::RepeatOff => "Repetir: no",
        Key::RepeatAll => "Repetir: todo",
        Key::RepeatOne => "Repetir: esta canción",
        Key::Previous => "Anterior",
        Key::Next => "Siguiente",
        Key::Volume => "Volumen",
        Key::ClearQueue => "Vaciar la cola",
        Key::NothingQueued => "Nada en la cola",
        Key::NothingQueuedBody => "Reproduce algo y aparecerá aquí.",
        Key::RemoveFromQueue => "Quitar de la cola",
        Key::TrackOptions => "Opciones de la canción",
        Key::PlayNext => "Reproducir _siguiente",
        Key::AddToQueue => "Añadir a la _cola",
        Key::AddToLibrary => "Añadir a la _biblioteca",
        Key::RemoveFromLibrary => "_Quitar de la biblioteca",
        Key::Favourite => "_Favorito",
        Key::RemoveFavourite => "Quitar _favorito",
        Key::GoToAlbum => "Ir al _álbum",
        Key::GoToArtist => "Ir al a_rtista",
        Key::Preferences => "_Preferencias",
        Key::KeyboardShortcuts => "Atajos de _teclado",
        Key::About => "_Acerca de Vinilo",
        Key::SignOut => "_Cerrar sesión",
        Key::Quit => "_Salir",
        Key::Appearance => "Apariencia",
        Key::Theme => "Tema",
        Key::ThemeSystem => "Seguir el sistema",
        Key::ThemeLight => "Claro",
        Key::ThemeDark => "Oscuro",
        Key::AccentColour => "Color de acento",
        Key::AccentAppleRed => "Rojo de Apple Music",
        Key::AccentSystem => "Seguir el sistema",
        Key::AccentBlue => "Azul",
        Key::AccentPurple => "Morado",
        Key::AccentGreen => "Verde",
        Key::AccentOrange => "Naranja",
        Key::AlbumArtBackdrop => "Color de la portada",
        Key::AlbumArtBackdropSub => {
            "Un tinte con el color principal del disco, detrás del reproductor"
        }
        Key::SourceTint => "Teñir según la fuente",
        Key::SourceTintSub => {
            "Rojo de Apple Music, verde de Spotify, rojo de YouTube, cian de Tidal"
        }
        Key::Language => "Idioma",
        Key::LanguageSub => "Idioma de la interfaz. Se aplica al momento.",
        Key::MusicSource => "Fuente de música",
        Key::MusicSourceSub => {
            "De dónde busca y reproduce Vinilo. Cambiar de fuente reinicia el reproductor."
        }
        Key::ProviderTitle => "¿Dónde está tu música?",
        Key::ProviderBody => {
            "Elige un catálogo. Apple Music es estable; Spotify y YouTube Music son Beta: funcionan, el sidecar de Apple sigue siendo pesado. Tidal es Alpha. Los archivos de este equipo siempre suenan."
        }
        Key::ProviderApple => "Apple Music",
        Key::ProviderAppleSub => {
            "Tu biblioteca y el catálogo. Funciona bien; el reproductor oculto sigue siendo Chromium."
        }
        Key::ProviderLocal => "Este equipo",
        Key::ProviderLocalSub => "Archivos que abras y carpetas que sueltes en Vinilo",
        Key::ProviderSpotify => "Spotify (Beta)",
        Key::ProviderSpotifySub => {
            "Biblioteca y búsqueda tras iniciar sesión. El audio nativo pide Spotify Premium."
        }
        Key::ProviderYoutube => "YouTube Music (Beta)",
        Key::ProviderYoutubeSub => {
            "Biblioteca y búsqueda tras iniciar sesión. El audio sale de YouTube Music."
        }
        Key::ProviderTidal => "Tidal (Alpha)",
        Key::ProviderTidalSub => "Busca tras iniciar sesión. La reproducción aún pasa por yt-dlp.",
        Key::ProviderNote => {
            "Puedes cambiarlo más tarde en Preferencias. Las fuentes Beta funcionan; Alpha se puede usar, pero no está lista. Spotify Premium suena con librespot; sin Premium, yt-dlp es el respaldo."
        }
        Key::ProviderRestart => "No se puede cambiar de fuente con otra ventana de Vinilo abierta.",
        Key::CatalogSetupBody => {
            "Inicia sesión para que Vinilo use tu sesión con este catálogo. Se abre una ventana, igual que con Apple Music."
        }
        Key::CatalogSetupYtOk => {
            "Cuando inicies sesión, busca una canción y pulsa. YouTube Music prefiere audio InnerTube; Tidal aún necesita yt-dlp."
        }
        Key::CatalogSetupYtMissing => {
            "yt-dlp no está en el PATH. YouTube Music sigue sonando por InnerTube cuando puede. Tidal necesita yt-dlp (sudo pacman -S yt-dlp ffmpeg)."
        }
        Key::CatalogSetupContinue => "Iniciar sesión",
        Key::CatalogSetupBack => "Elegir otra fuente",
        Key::CatalogSignInNote => {
            "La página de inicio de sesión se abre en una ventana aparte, incluida la verificación en dos pasos si tu cuenta la usa. Se cierra cuando ya estás dentro."
        }
        Key::CatalogLoginDone => "Listo",
        Key::CatalogOpenBrowser => "Abrir en el navegador",
        Key::CatalogSignOutTitle => "¿Cerrar sesión de este catálogo?",
        Key::CatalogSignOutBody => {
            "Vinilo olvidará esta sesión. Volver a entrar abre otra vez la ventana de inicio de sesión."
        }
        Key::SearchSpotifyBody => "Busca un tema de Spotify y pulsa para reproducirlo.",
        Key::SearchYoutubeBody => "Busca un tema de YouTube Music y pulsa para reproducirlo.",
        Key::SearchTidalBody => "Busca un tema de Tidal y pulsa para reproducirlo.",
        Key::EmptyLibraryBodyLocal => {
            "Abre un archivo de audio con Vinilo, suelta una carpeta en la ventana o pon Vinilo como reproductor de música predeterminado en Ajustes de GNOME."
        }
        Key::EmptyLibraryBodyCatalog => {
            "Aquí saldrán las canciones que te gustan, los álbumes y las listas de esta cuenta. \
             Si sigue vacío, vuelve a iniciar sesión desde el menú."
        }
        Key::Notifications => "Notificaciones",
        Key::NotifyTrackChange => "Avisar al cambiar de canción",
        Key::NotifyTrackChangeSub => {
            "Cuando empieza una canción nueva y Vinilo no está en primer plano"
        }
        Key::WelcomeTitle => "Bienvenido a Vinilo",
        Key::WelcomeBody => {
            "Vinilo reproduce tu biblioteca de Apple Music de forma nativa en GNOME. \
             Necesitas una suscripción activa a Apple Music."
        }
        Key::SignIn => "Iniciar sesión en Apple Music",
        Key::SignInNote => {
            "La página de inicio de sesión de Apple se abre en una ventana aparte, \
             incluida la verificación en dos pasos si tu cuenta la usa. Se cierra \
             del todo cuando ya estás dentro."
        }
        Key::SignOutTitle => "¿Cerrar sesión de Apple Music?",
        Key::SignOutBody => {
            "Vinilo olvidará esta sesión. Volver a entrar abre de nuevo la \
             ventana de inicio de sesión de Apple."
        }
        Key::Cancel => "Cancelar",
        Key::StartingEngine => "Arrancando el motor de reproducción",
        Key::Connecting => "Conectando con Apple Music",
        Key::PlaybackUnavailable => "Reproducción no disponible",
        Key::Ready => "Listo",
        Key::NothingPlaying => "Nada en reproducción",
        Key::SearchAppleMusic => "Buscar en Apple Music",
        Key::SearchAppleMusicBody => {
            "Encuentra canciones de todo el catálogo, no solo de tu biblioteca."
        }
        Key::SearchSpotify => "Buscar en Spotify",
        Key::SearchYoutube => "Buscar en YouTube Music",
        Key::SearchTidal => "Buscar en Tidal",
        Key::EmptyLibrary => "Todavía no hay nada aquí",
        Key::EmptyLibraryBody => {
            "Vinilo actualiza tu biblioteca después de iniciar sesión. Usa Recargar para intentarlo de nuevo. También puedes abrir un archivo de este equipo: Vinilo los reproduce también."
        }
        Key::NoMatches => "Sin coincidencias",
        Key::LoadingLibrary => "Cargando tu biblioteca",
        Key::LoadingAlbums => "Cargando tus álbumes",
        Key::LoadingArtists => "Cargando tus artistas",
        Key::LoadingPlaylists => "Cargando tus listas",
        Key::SearchingCatalog => "Buscando en Apple Music",
        Key::SearchLibrary => "Buscar en tu biblioteca",
        Key::SearchAlbums => "Buscar álbumes",
        Key::SearchArtists => "Buscar artistas",
        Key::SearchPlaylists => "Buscar listas",
        Key::SortTitle => "Título",
        Key::SortArtist => "Artista",
        Key::SortAlbum => "Álbum",
        Key::SortYear => "Año",
        Key::SortAdded => "Añadidos recientemente",
        Key::SortUpdated => "Actualizados recientemente",
        Key::ReverseOrder => "Orden _inverso",
        Key::FilterEverything => "Todo",
        Key::CouldNotLoadPage => "No se ha podido cargar esta página",
        Key::NothingHereYet => "Todavía no hay nada aquí",
        Key::PlayPage => "Reproducir",
        Key::PinPlaylists => "Fijar listas",
        Key::PinAPlaylist => "Fijar una lista",
        Key::PinAll => "Fijar todas",
        Key::UnpinAll => "Quitar todas",
        Key::AddToSidebar => "_Añadir a la barra lateral",
        Key::RemoveFromSidebar => "_Quitar de la barra lateral",
        Key::Unavailable => "No disponible",
        Key::NoPlaylistsYet => {
            "Tu biblioteca aún no tiene listas, o todavía no ha terminado de cargar."
        }
        Key::ShortcutsPlayback => "Reproducción",
        Key::ShortcutsGeneral => "General",
        Key::ShortcutPlayPause => "Reproducir o pausar",
        Key::ShortcutNext => "Canción siguiente",
        Key::ShortcutPrevious => "Canción anterior",
        Key::ShortcutVolumeUp => "Subir el volumen",
        Key::ShortcutVolumeDown => "Bajar el volumen",
        Key::ShortcutSearch => "Buscar",
        Key::ShortcutCloseWindow => "Cerrar la ventana",
        Key::ShortcutToggleSidebar => "Mostrar u ocultar la barra lateral",
        Key::ShortcutToggleQueue => "Mostrar u ocultar la cola",
        Key::ShortcutPreferences => "Preferencias",
        Key::ShortcutShortcuts => "Atajos de teclado",
        Key::ShortcutQuit => "Salir",
        Key::EarlierOne => "1 anterior",
        Key::HideEarlier => "Ocultar las canciones anteriores",
        Key::ShowEarlier => "Mostrar las canciones anteriores",
        Key::TracksOne => "1 canción",
        Key::Album => "álbum",
        Key::Artist => "artista",
        Key::Playlist => "lista",
        Key::Song => "canción",
        Key::SongsLower => "canciones",
        Key::EmptyAlbum => "Este álbum no tiene canciones.",
        Key::EmptyArtist => "Este artista no tiene álbumes.",
        Key::EmptyPlaylist => "Esta lista no tiene canciones.",
        Key::AboutComments => {
            "Un reproductor nativo de GNOME para Linux.\n\n\
             Apple Music pasa por el reproductor MusicKit de Apple con el CDM \
             Widevine de Google, en un proceso auxiliar oculto. Los archivos \
             de este equipo se reproducen de forma nativa. Spotify (Beta) suena \
             con librespot si tienes Premium; YouTube Music (Beta) prefiere \
             audio InnerTube; Tidal (Alpha) aún usa yt-dlp. Cada catálogo \
             guarda un inicio de sesión con cookies para biblioteca y búsqueda.\n\n\
             Fork de Slipmat, de Miguel Rincon."
        }
        Key::RefreshingLibrary => "Actualizando la biblioteca…",
        Key::ToastAddLibrary => "Añadiendo a tu biblioteca…",
        Key::ToastFavourite => "Marcando como favorito…",
        Key::ToastRemoveLibrary => "Quitando de tu biblioteca…",
        Key::ToastRemoveFavourite => "Quitando de favoritos…",
        Key::ToastGoneFromQueue => "Esa canción ya no está en la cola",
        Key::ToastUnstreamable => "Aquí no hay nada que se pueda reproducir",
        Key::LikedSongs => "Canciones que te gustan",
        Key::YourTopTracks => "Tus temas más reproducidos",
        Key::SpotifyNotSignedIn => {
            "Spotify no tiene sesión. Abre Iniciar sesión en el menú y entra otra vez."
        }
        Key::SpotifyTokenRefused => {
            "Spotify no ha emitido un token. Vuelve a iniciar sesión desde el menú si sigue pasando."
        }
        Key::CatalogLoginFailed => {
            "No se ha podido guardar la sesión. Quédate en la página ya iniciada y pulsa Listo."
        }
        Key::CatalogLibraryFailed => {
            "No se pudo cargar la biblioteca de este catálogo. Prueba Actualizar, o vuelve a iniciar sesión si esta fuente lo pide."
        }
        Key::SpotifyRateLimited => {
            "Spotify pide que esperemos. Prueba Recargar dentro de un minuto."
        }
        Key::SpotifyTimeout => "Spotify no ha contestado a tiempo. Prueba otra vez.",
        Key::SpotifyCatalogueChanged => {
            "La API del catálogo de Spotify ha cambiado. Vuelve a iniciar sesión y pulsa Recargar."
        }
        Key::NewPlaylist => "Nueva _lista",
        Key::NewPlaylistTitle => "Nueva lista",
        Key::NewPlaylistPlaceholder => "Nombre de la lista",
        Key::Create => "Crear",
        Key::AddToPlaylist => "Añadir a una _lista",
        Key::ToastCreatePlaylist => "Creando la lista…",
        Key::ToastAddToPlaylist => "Añadiendo a la lista…",
        Key::WriteWrongSource => "Esa canción pertenece a otra fuente de música.",
        Key::Lyrics => "Letra",
        Key::LyricsLoading => "Cargando la letra…",
        Key::LyricsMissing => "No hay letra para esta canción",
    }
}

/// English/Spanish templates that need a number or a query.
pub fn earlier_n(n: usize) -> String {
    match current() {
        Language::English => format!("{n} earlier"),
        Language::Spanish => format!("{n} anteriores"),
    }
}

pub fn tracks_n(n: usize) -> String {
    match current() {
        Language::English => format!("{n} tracks"),
        Language::Spanish => format!("{n} canciones"),
    }
}

pub fn catalog_search(provider: crate::provider::Provider) -> &'static str {
    t(match provider {
        crate::provider::Provider::Spotify => Key::SearchSpotify,
        crate::provider::Provider::YoutubeMusic => Key::SearchYoutube,
        crate::provider::Provider::Tidal => Key::SearchTidal,
        crate::provider::Provider::AppleMusic | crate::provider::Provider::Local => {
            Key::SearchAppleMusic
        }
    })
}

pub fn catalog_search_body(provider: crate::provider::Provider) -> &'static str {
    t(match provider {
        crate::provider::Provider::Spotify => Key::SearchSpotifyBody,
        crate::provider::Provider::YoutubeMusic => Key::SearchYoutubeBody,
        crate::provider::Provider::Tidal => Key::SearchTidalBody,
        crate::provider::Provider::AppleMusic | crate::provider::Provider::Local => {
            Key::SearchAppleMusicBody
        }
    })
}

pub fn catalog_heading(provider: crate::provider::Provider) -> &'static str {
    t(match provider {
        crate::provider::Provider::Spotify => Key::ProviderSpotify,
        crate::provider::Provider::YoutubeMusic => Key::ProviderYoutube,
        crate::provider::Provider::Tidal => Key::ProviderTidal,
        crate::provider::Provider::Local => Key::ProviderLocal,
        crate::provider::Provider::AppleMusic => Key::ProviderApple,
    })
}

pub fn catalog_sign_in(provider: crate::provider::Provider) -> String {
    let name = catalog_heading(provider);
    match current() {
        Language::English => format!("Sign In to {name}"),
        Language::Spanish => format!("Iniciar sesión en {name}"),
    }
}

pub fn searching_catalog(provider: crate::provider::Provider) -> String {
    let name = catalog_heading(provider);
    match current() {
        Language::English => format!("Searching {name}"),
        Language::Spanish => format!("Buscando en {name}"),
    }
}

pub fn no_library_songs(query: &str) -> String {
    match current() {
        Language::English => {
            format!("Nothing in your library matches “{query}”. Try searching Apple Music.")
        }
        Language::Spanish => {
            format!("Nada en tu biblioteca coincide con «{query}». Prueba a buscar en Apple Music.")
        }
    }
}

pub fn no_library_albums(query: &str) -> String {
    match current() {
        Language::English => format!("No album in your library matches “{query}”."),
        Language::Spanish => format!("Ningún álbum de tu biblioteca coincide con «{query}»."),
    }
}

pub fn no_library_artists(query: &str) -> String {
    match current() {
        Language::English => format!("No artist in your library matches “{query}”."),
        Language::Spanish => format!("Ningún artista de tu biblioteca coincide con «{query}»."),
    }
}

pub fn no_library_playlists(query: &str) -> String {
    match current() {
        Language::English => format!("No playlist in your library matches “{query}”."),
        Language::Spanish => format!("Ninguna lista de tu biblioteca coincide con «{query}»."),
    }
}

pub fn no_catalog(query: &str) -> String {
    no_catalog_for(query, crate::provider::load().unwrap_or_default())
}

pub fn no_catalog_for(query: &str, provider: crate::provider::Provider) -> String {
    let name = catalog_heading(provider);
    match current() {
        Language::English => format!("{name} has nothing matching “{query}”."),
        Language::Spanish => format!("{name} no tiene nada que coincida con «{query}»."),
    }
}

pub fn albums_count(n: usize) -> String {
    match current() {
        Language::English if n == 1 => "1 album".into(),
        Language::English => format!("{n} albums"),
        Language::Spanish if n == 1 => "1 álbum".into(),
        Language::Spanish => format!("{n} álbumes"),
    }
}

pub fn empty_album() -> &'static str {
    t(Key::EmptyAlbum)
}

pub fn empty_playlist() -> &'static str {
    t(Key::EmptyPlaylist)
}

pub fn empty_artist() -> &'static str {
    t(Key::EmptyArtist)
}

pub fn sign_out_button() -> &'static str {
    match current() {
        Language::English => "Sign Out",
        Language::Spanish => "Cerrar sesión",
    }
}

pub fn quit_button() -> &'static str {
    match current() {
        Language::English => "Quit",
        Language::Spanish => "Salir",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_round_trips() {
        for language in [Language::English, Language::Spanish] {
            assert_eq!(Language::parse(language.as_str()), Some(language));
            assert_eq!(Language::from_index(language.index()), language);
        }
    }

    #[test]
    fn unknown_language_is_none() {
        assert_eq!(Language::parse("fr"), None);
        assert_eq!(Language::parse(""), None);
    }

    #[test]
    fn spanish_welcome_is_spanish() {
        set_current(Language::Spanish);
        assert_eq!(t(Key::WelcomeTitle), "Bienvenido a Vinilo");
        assert_eq!(t(Key::SignIn), "Iniciar sesión en Apple Music");
        set_current(Language::English);
        assert_eq!(t(Key::WelcomeTitle), "Welcome to Vinilo");
    }

    #[test]
    fn empty_states_are_complete_sentences() {
        set_current(Language::English);
        assert_eq!(empty_album(), "This album has no songs.");
        assert_eq!(empty_playlist(), "This playlist has no songs.");
        assert_eq!(empty_artist(), "This artist has no albums.");
        set_current(Language::Spanish);
        assert_eq!(empty_album(), "Este álbum no tiene canciones.");
        assert_eq!(empty_playlist(), "Esta lista no tiene canciones.");
        assert_eq!(empty_artist(), "Este artista no tiene álbumes.");
        set_current(Language::English);
    }
}
