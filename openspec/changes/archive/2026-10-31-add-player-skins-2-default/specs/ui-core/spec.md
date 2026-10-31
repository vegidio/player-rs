## MODIFIED Requirements

### Requirement: User actions

The UI layer SHALL perform these actions on the player:
- play, pause, toggle play
- seek to a position, seek by an offset
- toggle mute
- set the volume, step the volume by an amount
- set the rate
- toggle fullscreen
- retry after an error
- toggle the time display
- open, close and navigate the Settings menu

Toggle mute SHALL follow Video.js: at volume 0, it SHALL set the volume to the last non-zero volume (at least 0.1) and unmute. Otherwise it SHALL flip the mute state. Stepping or setting the volume while muted SHALL unmute. Choosing among the configured playback rates SHALL happen on the Settings menu's Speed page; there SHALL be no action that cycles through them. Toggling fullscreen SHALL not touch the player; it SHALL be returned to the frontend as a request, because only the frontend controls its window.

#### Scenario: Toggle mute at volume zero
- **WHEN** the volume was 0.6, was then set to 0, and mute is toggled
- **THEN** the volume becomes 0.6 and the player is unmuted

#### Scenario: Toggle mute from a tiny volume
- **WHEN** the last non-zero volume was 0.05, the volume is 0, and mute is toggled
- **THEN** the volume becomes 0.1

#### Scenario: Set the rate
- **WHEN** the rate is set to 1.5
- **THEN** the player's rate becomes 1.5

#### Scenario: Cycle the rate
- **WHEN** the playback rates are `[0.5, 1, 1.5, 2]` and the rate is 2
- **THEN** no action moves to the next rate; 0.5 is chosen by selecting it on the Speed page, and the rate becomes 0.5

#### Scenario: Cycle from a rate not in the list
- **WHEN** the playback rates are `[0.5, 1, 2]`, the rate is 1.25, and `0.5x` is selected on the Speed page
- **THEN** the rate becomes 0.5

#### Scenario: Fullscreen is the frontend's
- **WHEN** fullscreen is toggled
- **THEN** a fullscreen request is returned to the frontend and the player's state does not change

### Requirement: Playback rate control

The playback rate SHALL be offered as the Speed item of the Settings menu, only when playback rates are configured and the Speed part is visible. Its value label SHALL be the current rate followed by `x`, with no trailing zeros (`1x`, `1.5x`). Its page SHALL list every configured rate in order, with the current rate checked. Selecting one SHALL set that rate.

#### Scenario: No rates
- **WHEN** no playback rates are configured
- **THEN** the Speed item is not offered

#### Scenario: Speed hidden
- **WHEN** playback rates are configured but the Speed part is hidden
- **THEN** the Speed item is not offered

#### Scenario: Label and cycle
- **WHEN** the playback rates are `[1, 1.5, 2]`, the rate is 1, and the rate is set to 1.5 without the menu
- **THEN** the Speed item's value reads `1.5x` and `1.5x` is checked on its page

#### Scenario: Label and selection
- **WHEN** the playback rates are `[1, 1.5, 2]`, the rate is 1, and `1.5x` is selected on the Speed page
- **THEN** the rate becomes 1.5, the Speed item's value reads `1.5x` and `1.5x` is checked

#### Scenario: Rate set elsewhere
- **WHEN** the playback rates are `[1, 1.5, 2]` and the rate is set to 1.25
- **THEN** the Speed item's value reads `1.25x` and no entry on its page is checked
