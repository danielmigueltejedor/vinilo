<!--
SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
SPDX-FileCopyrightText: 2026 Miguel Rincon
SPDX-License-Identifier: GPL-3.0-or-later
-->

<p align="center">
  <img src="docs/screenshots/icon.png" width="128" alt="Icono de Vinilo">
</p>

# Vinilo

Reproductor nativo de música para Linux, con interfaz en **inglés** o **español**.

Vinilo es un fork de [Slipmat](https://github.com/SoftARV/Slipmat) de Miguel Rincon. En el primer arranque eliges **de dónde sale la música**: Apple Music o los archivos de este equipo. Spotify, YouTube Music y Tidal están en el selector porque son el rumbo; hoy no transmiten.

Apple no publica cliente para Linux, y Widevine impide una pila 100 % nativa para el catálogo. Vinilo dibuja su propia interfaz en Rust (GTK4 / libadwaita) y deja el motor web oculto solo para descifrar Apple Music. Los archivos locales se reproducen en el propio demonio, sin Chromium.

La primera vez eliges idioma y fuente. Luego puedes cambiarlos en **Preferencias** (`Ctrl`+`,`).

<p align="center">
  <img src="docs/screenshots/library.webp" alt="Vinilo mostrando la biblioteca de canciones">
</p>

## Qué hace

- **Dos clientes nativos.** La aplicación GNOME o `aguja` en la terminal. Ambos controlan el mismo motor.
- **Toda tu biblioteca.** Canciones, álbumes, artistas y listas. Reproducir una lista la convierte en la cola.
- **Reproducción sin cortes** y reproductor a pantalla completa con portada y cola.
- **Controles del escritorio.** Barra superior de GNOME, pantalla de bloqueo o teclas multimedia. La música sigue si cierras la ventana.
- **Búsqueda en Apple Music.** Artistas, álbumes, listas y canciones.
- **Escuchar ahora.** Recién reproducido, listas hechas para ti y éxitos, con Apple Music si responde y con tu biblioteca si no.
- **Listas rápidas.** Las playlists que ya abriste se quedan en caché; las filas muestran la carátula de cada canción.
- **Archivos de este equipo.** Ábrelo desde Archivos, suelta una carpeta, o pon Vinilo como reproductor predeterminado. Funciona con Apple Music y también en solitario.
- **Idioma y fuente en Preferencias.** Inglés y español; Apple Music o este equipo.

## Requisitos

Para **Apple Music** necesitas una suscripción activa, una máquina **x86_64** (Widevine en Linux solo existe ahí) y red cada vez que reproduces. Esa instalación descarga una vez castLabs Electron (~200 MB de Chromium) para el CDM Widevine.

Para **archivos locales** basta con el propio Vinilo: MP3, FLAC, Ogg, WAV y demás que rodio sepa abrir. No hace falta sidecar ni cuenta.

## Compilar e instalar

El `Makefile` vive **dentro del repositorio**. Si ejecutas `make install` desde `~` verás:

```
make: *** No hay ninguna regla para construir el objetivo 'install'.  Alto.
```

Eso no es un fallo de pacman: no hay Makefile en el directorio de trabajo.

Primero publica el código en GitHub (en Cursor: **Create repo**). Hasta que ese remoto exista, `git clone` fallará: el proyecto vive aquí, no todavía en github.com.

GitHub **no acepta la contraseña de la cuenta** para `git clone` / `git push`. Usa SSH o un token (PAT), no la contraseña de iCloud/GitHub.

```bash
# 1. Clona con la URL que te dé GitHub (SSH, no HTTPS+contraseña)
git clone git@github.com:TU_USUARIO/vinilo.git
cd vinilo

# 2. Dependencias (Arch). No hace falta sudo para make.
sudo pacman -S --needed base-devel pkgconf rust gtk4 libadwaita librsvg \
                        nodejs npm libpulse alsa-lib

# 3. Compila, descarga el sidecar (~200 MB) e instala en ~/.local
make install
```

Arranca **Vinilo** desde la parrilla de aplicaciones o:

```bash
# fish (Arch por defecto a menudo no incluye ~/.local/bin)
fish_add_path ~/.local/bin
vinilo
```

`make install` escribe `Exec=/home/…/.local/bin/vinilo` en el `.desktop` y un servicio D-Bus con la misma ruta. Un `Exec=vinilo` a secas es por lo que la parrilla no abría nada: el PATH de GNOME no incluye `~/.local/bin`, y el de fish sí.

Si `git pull` aborta por `Cargo.lock` (un `cargo build` local lo ensucia), no instales a medias. Desde `~/vinilo`:

```bash
make update
```

Eso descarta el `Cargo.lock` local, trae `main` y vuelve a instalar. Equivale a:

```bash
git restore -- Cargo.lock
git pull
pkill vinilod
make install
```

Si tras instalar la parrilla sigue muda: cierra sesión y vuelve a entrar, o `update-desktop-database ~/.local/share/applications`. El `.desktop` tiene que decir `Exec=/home/TU_USUARIO/.local/bin/vinilo`, no `vinilo`.

Para que sea el reproductor predeterminado: en Archivos, clic derecho en una canción → **Abrir con** → Vinilo → **Siempre**. O:

```bash
xdg-mime default dev.danielmiguelt.Vinilo.desktop audio/mpeg
xdg-mime default dev.danielmiguelt.Vinilo.desktop audio/flac
xdg-mime default dev.danielmiguelt.Vinilo.desktop audio/ogg
```

`make install` **no lleva sudo**: instala en `~/.local`. La primera vez tarda un rato (Rust en release + Chromium del sidecar si usas Apple Music).

Si ya tenías Vinilo abierto, cierra la aplicación y para el demonio viejo para que coja el protocolo nuevo:

```bash
pkill vinilod
```

`aguja` se instala junto a Vinilo: el reproductor en terminal. También necesita una sesión gráfica porque el demonio ejecuta Chromium.

```bash
cargo run                                    # la app GNOME
cargo run -p aguja                           # el reproductor de terminal
RUST_LOG=vinilod=debug cargo run -p vinilod  # el motor
make sidecar-run                             # sidecar solo, ventana VISIBLE
make check                                   # fmt + clippy + test
```

## Primer arranque

1. Elige **English** o **Español**.
2. Elige de dónde sale la música: **Apple Music** o **este equipo**. Spotify, YouTube Music y Tidal aparecen como «próximamente».
3. Si elegiste Apple Music, inicia sesión. Se abre la página de Apple en una ventana aparte (incluido el 2FA). Después se oculta.
4. La biblioteca de Apple aparece desde la caché en los siguientes arranques. Los archivos locales se reproducen en cuanto los abres.

Una instalación que ya tenía idioma elegido no vuelve a preguntar la fuente: se queda en Apple Music, que es lo que ya usabas.

El idioma queda en `~/.config/vinilo/locale` y la fuente en `~/.config/vinilo/provider`.

## Cómo funciona

```
┌────────────────────────────┐   ┌────────────────────────────┐
│  Vinilo: GTK4/libadwaita   │   │  aguja: la terminal        │
│  biblioteca · búsqueda     │   │  lo mismo, en texto        │
└─────────────┬──────────────┘   └─────────────┬──────────────┘
              │      JSON por un socket Unix
              └──────────────┬─────────────────┘
┌────────────────────────────▼─────────────────────────────────┐
│  vinilod: el motor                            ← sin ventana  │
│  cola · MPRIS · portadas · archivos locales · caché          │
└──────────────┬──────────────────────────────┬────────────────┘
               │ Apple Music                  │ archivos
┌──────────────▼──────────────┐    ┌──────────▼────────────────┐
│  sidecar: castLabs Electron │    │  rodio en el propio motor │
│  MusicKit + Widevine        │    │  MP3, FLAC, Ogg, WAV…     │
└─────────────────────────────┘    └───────────────────────────┘
```

Apple Music pasa por el reproductor MusicKit de Apple con el CDM oficial de Google. Los archivos de este equipo no. Vinilo no quita el DRM ni descarga pistas.

## Limitaciones

- **Sin reproducción sin conexión de Apple Music.** El CDM de Linux no admite licencias persistentes.
- **~200 MB en disco** para el sidecar de Chromium, solo si usas Apple Music.
- **Solo x86_64** para Apple Music, mientras Widevine en Linux ARM no esté estable.
- **Spotify, YouTube Music y Tidal** están en el selector y aún no reproducen.
- **`aguja` necesita una sesión de escritorio** si usas Apple Music: Chromium pide un servidor de pantalla.

## Créditos

Fork de [Slipmat](https://github.com/SoftARV/Slipmat) (GPL-3.0-or-later) de Miguel Rincon. [Sidra](https://github.com/wimpysworld/sidra) y [Cider](https://cider.sh) abrieron el camino de castLabs Electron para Apple Music en Linux.

## Licencia

GPL-3.0-or-later. Ver [COPYING](COPYING).
