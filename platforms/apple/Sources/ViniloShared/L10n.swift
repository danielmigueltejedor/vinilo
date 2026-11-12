// SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// The interface's words, in the two languages Vinilo ships. Spanish is the product's own
/// language; everything else gets English.
public enum L10n {
    public enum Key: String, CaseIterable, Sendable {
        case appName
        case home, search, library, artists, albums, songs, playlists, services
        case appleMusic, lyrics, noLyrics
        case source, sourceBody, localFiles, openFiles, localBody, changeSource, signInAgain
        case loginTitle, loginBody, cancel, beta, alpha, needsYtDlp, searchIn, emptyLibraryAny
        case playPause, play, pause, next, previous, shuffle, repeatMode, volume, queue
        case nowPlaying, nothingPlaying, upNext, queueEmpty, clearQueue
        case playAll, shuffleAll, addToQueue, playNext
        case connecting, signedOutTitle, signedOutBody, signIn, retry, brokenTitle
        case engineLost, engineLostBody
        case emptyLibrary, emptyLibraryBody, emptyResults, searchPrompt, searchHint
        case loading, refresh, refreshing, dismiss
        case recentlyPlayed, madeForYou, recentlyAdded, charts, topSongs
        case unavailable, tracks, album, artist, playlist, song
        case notifications, notificationsBody, edition, editionBody
        case close, expand, progress
        case playback, volumeUp, volumeDown, signOut, reloadLibrary, settingsLibrary
    }

    private static let english: [Key: String] = [
        .appName: "Vinilo",
        .home: "Home", .search: "Search", .library: "Library", .artists: "Artists",
        .albums: "Albums", .songs: "Songs", .playlists: "Playlists", .services: "Services",
        .appleMusic: "Apple Music", .lyrics: "Lyrics", .noLyrics: "No lyrics for this song",
        .source: "Music source",
        .sourceBody: "Vinilo plays from one source at a time. Changing it restarts the engine.",
        .localFiles: "Files on this Mac", .openFiles: "Open Files…",
        .localBody: "Choose songs or folders to play them.",
        .changeSource: "Change source…", .signInAgain: "Sign in again",
        .loginTitle: "Sign in to %@",
        .loginBody: "Once you are in, Vinilo keeps the session and closes this window.",
        .cancel: "Cancel", .beta: "Beta", .alpha: "Alpha",
        .needsYtDlp: "Plays through yt-dlp and ffmpeg from Homebrew: brew install yt-dlp ffmpeg",
        .searchIn: "Search %@",
        .emptyLibraryAny: "What you save in your library appears here.",
        .playPause: "Play or pause", .play: "Play", .pause: "Pause", .next: "Next",
        .previous: "Previous", .shuffle: "Shuffle", .repeatMode: "Repeat", .volume: "Volume",
        .queue: "Queue",
        .nowPlaying: "Now Playing", .nothingPlaying: "Nothing playing", .upNext: "Up next",
        .queueEmpty: "The queue is empty", .clearQueue: "Clear queue",
        .playAll: "Play", .shuffleAll: "Shuffle", .addToQueue: "Add to queue",
        .playNext: "Play next",
        .connecting: "Starting the engine…", .signedOutTitle: "Sign in to Apple Music",
        .signedOutBody:
            "Vinilo needs an active Apple Music subscription. Apple's own sign-in page opens in a separate window.",
        .signIn: "Sign in", .retry: "Try again", .brokenTitle: "Vinilo cannot play right now",
        .engineLost: "Lost the engine",
        .engineLostBody: "Vinilo is starting it again. Playback may pause for a moment.",
        .emptyLibrary: "Nothing here yet",
        .emptyLibraryBody: "What you add to your Apple Music library appears here.",
        .emptyResults: "No results", .searchPrompt: "Search Apple Music",
        .searchHint: "Songs, albums, artists and playlists",
        .loading: "Loading…", .refresh: "Reload library", .refreshing: "Updating the library…",
        .dismiss: "Dismiss",
        .recentlyPlayed: "Recently played", .madeForYou: "Made for you",
        .recentlyAdded: "Recently added", .charts: "Charts", .topSongs: "Recommended songs",
        .unavailable: "Not available to stream", .tracks: "tracks", .album: "Album",
        .artist: "Artist", .playlist: "Playlist", .song: "Song",
        .notifications: "Notify when the song changes",
        .notificationsBody: "Only while Vinilo is in the background. macOS asks the first time.",
        .edition: "Vinilo Native",
        .editionBody:
            "This is the SwiftUI edition. Vinilo Nodalia, the other edition, can stay installed beside it; each keeps its own settings.",
        .close: "Close", .expand: "Open Now Playing", .progress: "Playback position",
        .playback: "Playback", .volumeUp: "Volume up", .volumeDown: "Volume down",
        .signOut: "Sign out of Apple Music", .reloadLibrary: "Reload the library",
        .settingsLibrary: "Account and library",
    ]

    private static let spanish: [Key: String] = [
        .appName: "Vinilo",
        .home: "Inicio", .search: "Buscar", .library: "Biblioteca", .artists: "Artistas",
        .albums: "Álbumes", .songs: "Canciones", .playlists: "Listas de reproducción",
        .services: "Servicios",
        .appleMusic: "Apple Music", .lyrics: "Letra", .noLyrics: "Esta canción no tiene letra",
        .source: "Fuente de música",
        .sourceBody: "Vinilo reproduce de una fuente a la vez. Al cambiarla se reinicia el motor.",
        .localFiles: "Archivos de este Mac", .openFiles: "Abrir archivos…",
        .localBody: "Elige canciones o carpetas para reproducirlas.",
        .changeSource: "Cambiar de fuente…", .signInAgain: "Volver a iniciar sesión",
        .loginTitle: "Inicia sesión en %@",
        .loginBody: "Cuando entres, Vinilo guarda la sesión y cierra esta ventana.",
        .cancel: "Cancelar", .beta: "Beta", .alpha: "Alfa",
        .needsYtDlp: "Suena a través de yt-dlp y ffmpeg de Homebrew: brew install yt-dlp ffmpeg",
        .searchIn: "Buscar en %@",
        .emptyLibraryAny: "Lo que guardes en tu biblioteca aparece aquí.",
        .playPause: "Reproducir o pausar", .play: "Reproducir", .pause: "Pausar",
        .next: "Siguiente", .previous: "Anterior", .shuffle: "Aleatorio", .repeatMode: "Repetir",
        .volume: "Volumen", .queue: "Cola",
        .nowPlaying: "En reproducción", .nothingPlaying: "No suena nada",
        .upNext: "A continuación", .queueEmpty: "La cola está vacía", .clearQueue: "Vaciar la cola",
        .playAll: "Reproducir", .shuffleAll: "Aleatorio", .addToQueue: "Añadir a la cola",
        .playNext: "Reproducir a continuación",
        .connecting: "Iniciando el motor…", .signedOutTitle: "Inicia sesión en Apple Music",
        .signedOutBody:
            "Vinilo necesita una suscripción activa de Apple Music. La página de inicio de sesión de Apple se abre en una ventana aparte.",
        .signIn: "Iniciar sesión", .retry: "Reintentar",
        .brokenTitle: "Vinilo no puede reproducir ahora",
        .engineLost: "Se perdió el motor",
        .engineLostBody: "Vinilo lo está iniciando de nuevo. La reproducción puede pausarse un momento.",
        .emptyLibrary: "Aquí aún no hay nada",
        .emptyLibraryBody: "Lo que añadas a tu biblioteca de Apple Music aparece aquí.",
        .emptyResults: "Sin resultados", .searchPrompt: "Buscar en Apple Music",
        .searchHint: "Canciones, álbumes, artistas y listas",
        .loading: "Cargando…", .refresh: "Recargar la biblioteca",
        .refreshing: "Actualizando la biblioteca…", .dismiss: "Descartar",
        .recentlyPlayed: "Reproducido recientemente", .madeForYou: "Para ti",
        .recentlyAdded: "Añadido recientemente", .charts: "Listas de éxitos",
        .topSongs: "Canciones recomendadas",
        .unavailable: "No disponible para reproducir", .tracks: "canciones", .album: "Álbum",
        .artist: "Artista", .playlist: "Lista", .song: "Canción",
        .notifications: "Avisar cuando cambie la canción",
        .notificationsBody:
            "Solo cuando Vinilo está en segundo plano. macOS lo pregunta la primera vez.",
        .edition: "Vinilo Native",
        .editionBody:
            "Esta es la edición SwiftUI. Vinilo Nodalia, la otra edición, puede seguir instalada a su lado; cada una guarda sus propios ajustes.",
        .close: "Cerrar", .expand: "Abrir En reproducción", .progress: "Posición de reproducción",
        .playback: "Reproducción", .volumeUp: "Subir volumen", .volumeDown: "Bajar volumen",
        .signOut: "Cerrar sesión de Apple Music", .reloadLibrary: "Recargar la biblioteca",
        .settingsLibrary: "Cuenta y biblioteca",
    ]

    public static func t(_ key: Key, languageCode: String? = Locale.current.language.languageCode?.identifier)
        -> String
    {
        let table = languageCode == "es" ? spanish : english
        return table[key] ?? english[key] ?? key.rawValue
    }

    /// `3:05`, or `1:02:03`. Anything negative or not a number reads as zero.
    public static func clock(ms: Int) -> String {
        let total = max(0, ms) / 1000
        let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60)
        return hours > 0
            ? String(format: "%d:%02d:%02d", hours, minutes, seconds)
            : String(format: "%d:%02d", minutes, seconds)
    }
}
