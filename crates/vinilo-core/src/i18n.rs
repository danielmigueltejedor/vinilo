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
    Library,
    All,
    Queue,
    AppleMusic,
    MainMenu,
    ToggleSidebar,
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
    Language,
    LanguageSub,
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
        Key::Library => "Library",
        Key::All => "All",
        Key::Queue => "Queue",
        Key::AppleMusic => "Apple Music",
        Key::MainMenu => "Main Menu",
        Key::ToggleSidebar => "Toggle Sidebar",
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
        Key::AlbumArtBackdrop => "Album Art Backdrop",
        Key::AlbumArtBackdropSub => "The current cover, blurred, behind the player",
        Key::Language => "Language",
        Key::LanguageSub => "Interface language. Takes effect immediately.",
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
        Key::SearchAppleMusicBody => {
            "Find songs from the whole catalogue, not just your library."
        }
        Key::EmptyLibrary => "Nothing here yet",
        Key::EmptyLibraryBody => {
            "Vinilo refreshes your library after sign-in. Use Reload to try again."
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
        Key::NoPlaylistsYet => {
            "Your library has no playlists yet, or it has not finished loading."
        }
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
        Key::EmptyAlbum => "This {kind} has no songs.",
        Key::EmptyArtist => "No matches in your library for “{query}”.",
        Key::EmptyPlaylist => "No playlist in your library matches “{query}”.",
        Key::AboutComments => {
            "A native GNOME client for Apple Music.\n\n\
             Playback runs through Apple's own MusicKit player using Google's \
             Widevine CDM, in a hidden helper process. Vinilo is a native \
             front-end for a licensed session — it requires an active Apple \
             Music subscription and an internet connection.\n\n\
             Fork of Slipmat by Miguel Rincon."
        }
        Key::RefreshingLibrary => "Refreshing library…",
        Key::ToastAddLibrary => "Adding to your library…",
        Key::ToastFavourite => "Favouriting…",
        Key::ToastRemoveLibrary => "Removing from your library…",
        Key::ToastRemoveFavourite => "Removing favourite…",
        Key::ToastGoneFromQueue => "That track is no longer in the queue",
        Key::ToastUnstreamable => "Nothing here can be streamed",
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
        Key::Library => "Biblioteca",
        Key::All => "Todas",
        Key::Queue => "Cola",
        Key::AppleMusic => "Apple Music",
        Key::MainMenu => "Menú principal",
        Key::ToggleSidebar => "Mostrar u ocultar la barra lateral",
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
        Key::AlbumArtBackdrop => "Fondo con la portada",
        Key::AlbumArtBackdropSub => "La portada actual, desenfocada, detrás del reproductor",
        Key::Language => "Idioma",
        Key::LanguageSub => "Idioma de la interfaz. Se aplica al momento.",
        Key::Notifications => "Notificaciones",
        Key::NotifyTrackChange => "Avisar al cambiar de canción",
        Key::NotifyTrackChangeSub => "Cuando empieza una canción nueva y Vinilo no está en primer plano",
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
        Key::EmptyLibrary => "Todavía no hay nada aquí",
        Key::EmptyLibraryBody => {
            "Vinilo actualiza tu biblioteca después de iniciar sesión. Usa Recargar para intentarlo de nuevo."
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
        Key::EmptyAlbum => "Este {kind} no tiene canciones.",
        Key::EmptyArtist => "Nada en tu biblioteca coincide con «{query}».",
        Key::EmptyPlaylist => "Ninguna lista de tu biblioteca coincide con «{query}».",
        Key::AboutComments => {
            "Un cliente nativo de GNOME para Apple Music.\n\n\
             La reproducción pasa por el reproductor MusicKit de Apple con el CDM \
             Widevine de Google, en un proceso auxiliar oculto. Vinilo es una \
             interfaz nativa para una sesión con licencia: necesita una \
             suscripción activa a Apple Music y conexión a internet.\n\n\
             Fork de Slipmat, de Miguel Rincon."
        }
        Key::RefreshingLibrary => "Actualizando la biblioteca…",
        Key::ToastAddLibrary => "Añadiendo a tu biblioteca…",
        Key::ToastFavourite => "Marcando como favorito…",
        Key::ToastRemoveLibrary => "Quitando de tu biblioteca…",
        Key::ToastRemoveFavourite => "Quitando de favoritos…",
        Key::ToastGoneFromQueue => "Esa canción ya no está en la cola",
        Key::ToastUnstreamable => "Aquí no hay nada que se pueda reproducir",
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

pub fn no_library_songs(query: &str) -> String {
    match current() {
        Language::English => format!(
            "Nothing in your library matches “{query}”. Try searching Apple Music."
        ),
        Language::Spanish => format!(
            "Nada en tu biblioteca coincide con «{query}». Prueba a buscar en Apple Music."
        ),
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
    match current() {
        Language::English => format!("Apple Music has nothing matching “{query}”."),
        Language::Spanish => format!("Apple Music no tiene nada que coincida con «{query}»."),
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
    match current() {
        Language::English => "This album has no songs.",
        Language::Spanish => "Este álbum no tiene canciones.",
    }
}

pub fn empty_playlist() -> &'static str {
    match current() {
        Language::English => "This playlist has no songs.",
        Language::Spanish => "Esta lista no tiene canciones.",
    }
}

pub fn empty_artist() -> &'static str {
    match current() {
        Language::English => "This artist has no albums.",
        Language::Spanish => "Este artista no tiene álbumes.",
    }
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
}
