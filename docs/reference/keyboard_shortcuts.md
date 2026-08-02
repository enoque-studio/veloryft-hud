# Keyboard shortcuts and the tray menu

## Global shortcuts

| Shortcut | Action |
| -------- | ------ |
| **Ctrl + Alt + E** | Enter or leave edit mode |
| **Ctrl + Alt + H** | Hide or show the overlay |
| **Ctrl + Alt + Q** | Quit Veloryft |

These are registered with Windows as global hotkeys, so they work while the game
has focus. They are not configurable.

Windows grants a hotkey to the first program that asks. If another application
already holds one of these combinations, Veloryft does not get it and the
shortcut does nothing — close the other program and restart Veloryft.

`Ctrl+Alt+E` also clears the hidden state, so it brings the overlay back if
`Ctrl+Alt+H` had hidden it.

## Tray icon

| Action | Result |
| ------ | ------ |
| Double-click | Show or hide the overlay |
| Right-click → Show/Hide | Same |
| Right-click → Exit | Quit, saving settings first |

New tray icons land under the hidden-icons arrow next to the clock until you
drag them onto the taskbar.

## Edit mode

Edit mode is the only state in which the overlay accepts mouse input; the rest
of the time clicks pass through to the game.

| Action | Result |
| ------ | ------ |
| Drag the middle of a panel | Move it |
| Drag an edge or corner | Resize, preserving the aspect ratio |
| Mouse wheel over the toolbar swatches | Change the accent hue |
| Right-click the toolbar | Switch the shift highlight between the gear and the card |
| Click a panel name on the toolbar | Show or hide that panel |
| Drag the **GLASS** slider | Card background opacity |
| Drag the **TONE** slider | Glass tint; leftmost is neutral dark |

Panels snap to a 40-pixel grid when within 12 pixels of a line, and otherwise
stay exactly where you drop them. A panel cannot be resized below 80 pixels on
either axis.

Every change is saved as you make it. See
[configuration files](configuration_files.md).
