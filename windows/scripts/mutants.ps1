# Proves the model checks can fail. Each mutant breaks one property in the real ticker-core source, runs the model
# checks, and expects the named invariant to break; the source is restored afterwards. Prints one PASS or FAIL line
# per mutant and exits 0 only when every mutant behaved as expected. Runs on Windows PowerShell 5.1.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File windows\scripts\mutants.ps1 [-Only <name>]
#
# A mutant whose Find text no longer occurs exactly once fails with "stale": update its Find to the current code.
# Mutants build with lints capped at warn in their own target dir, so an unused variable a mutant leaves behind does
# not stop the build, and the normal build cache stays untouched.
param([string]$Only)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$crate = Join-Path $workspace 'crates\ticker-core'
# A killed run skips the restore below and leaves its mutant in the source; mutating on top of it would hide that.
$dirty = git -C $crate status --porcelain -- src
if ($dirty) {
    Write-Output "FAIL         ticker-core src has uncommitted changes, maybe a mutant a killed run left behind:"
    $dirty | ForEach-Object { "    | $_" }
    exit 1
}
$logs = Join-Path $workspace 'target\verify\mutants'
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding $false
$env:RUSTFLAGS = '--cap-lints=warn'
$env:CARGO_TARGET_DIR = Join-Path $workspace 'target\mutants-build'

$mutants = @(
    @{ Name = 'quote-in-flight-guard-dropped'; File = 'src\app.rs'; Expect = 'never two quote requests outstanding'
        Edits = @(@{ Find = 'if self.quote_in_flight.is_none() && self.schedule.is_due(refresh_seconds, self.now) {'
                Replace = 'if self.schedule.is_due(refresh_seconds, self.now) {' }) }
    @{ Name = 'stale-quote-id-accepted'; File = 'src\app.rs'; Expect = 'a late or unknown result changes nothing'
        Edits = @(@{ Find = 'if self.quote_in_flight != Some(request) {'; Replace = 'if self.quote_in_flight.map(|_| request).is_none() {' }) }
    @{ Name = 'stale-candle-id-accepted-while-loading'; File = 'src\app.rs'; Expect = 'a late or unknown result changes nothing'
        Edits = @(@{ Find = 'if !self.charts.awaits(timeframe, request) {'; Replace = 'if !self.chart_load(timeframe).is_loading() {' }) }
    @{ Name = 'request-ids-reused'; File = 'src\app.rs'; Expect = 'a late or unknown result changes nothing'
        Edits = @(@{ Find = 'self.requests_issued += 1;'; Replace = '' }) }
    @{ Name = 'candles-fetched-while-hidden'; File = 'src\chart.rs'; Expect = 'candles are fetched only for the visible expanded timeframe'
        Edits = @(@{ Find = '(overlay.is_visible() && !load.is_loading())'; Replace = '(!load.is_loading())' }) }
    @{ Name = 'candles-fetched-while-collapsed'; File = 'src\chart.rs'; Expect = 'candles are fetched only for the visible expanded timeframe'
        Edits = @(@{ Find = 'let OverlayMode::Expanded(timeframe) = overlay.mode() else { return None };'
                Replace = 'let OverlayMode::Expanded(timeframe) = OverlayMode::Expanded(overlay.settings().timeframe) else { return None };' }) }
    @{ Name = 'candle-in-flight-guard-dropped'; File = 'src\chart.rs'; Expect = 'never two candle requests outstanding for one timeframe'
        Edits = @(@{ Find = '(overlay.is_visible() && !load.is_loading())'; Replace = '(overlay.is_visible())' }) }
    @{ Name = 'candle-failure-kept-after-success'; File = 'src\chart.rs'; Expect = "the visible expanded chart's candles are requested exactly when due"
        Edits = @(@{ Find = 'load.failure = None;'; Replace = '' }) }
    @{ Name = 'refresh-edit-not-rescheduled'; File = 'src\app.rs'; Expect = 'a quote request starts exactly when one is due and none is out'
        Edits = @(@{ Find = 'if self.quote_in_flight.is_none() && self.schedule.is_due(refresh_seconds, self.now) {'
                Replace = 'if self.quote_in_flight.is_none() && self.schedule.is_due(refresh_seconds, self.now) && !edited_in_app {' }) }
    @{ Name = 'wake-keeps-backoff'; File = 'src\app.rs'; Expect = 'a quote request starts exactly when one is due and none is out'
        Edits = @(@{ Find = 'self.schedule.woke();'; Replace = '' }) }
    @{ Name = 'tick-does-not-advance-time'; File = 'src\app.rs'; Expect = 'the feed status is FeedStatus::derive of the last good quote, the start and the clock'
        Edits = @(@{ Find = 'Input::Tick(now) => self.advance(now),'; Replace = 'Input::Tick(_) => {}' }) }
    @{ Name = 'settings-file-written-back'; File = 'src\app.rs'; Expect = 'SaveSettings is emitted iff an in-app input changed the settings, and carries the new settings'
        Edits = @(@{ Find = 'if edited_in_app && self.overlay.settings() != overlay_before.settings() {'
                Replace = 'if self.overlay.settings() != overlay_before.settings() {' }) }
    @{ Name = 'saves-unchanged-settings'; File = 'src\app.rs'; Expect = 'SaveSettings is emitted iff an in-app input changed the settings, and carries the new settings'
        Edits = @(@{ Find = 'if edited_in_app && self.overlay.settings() != overlay_before.settings() {'; Replace = 'if edited_in_app {' }) }
    @{ Name = 'no-render-for-chart-loads'; File = 'src\app.rs'; Expect = 'Render is emitted iff the overlay, the feed status, the shown text or a chart load changed'
        Edits = @(@{ Find = 'if charts_changed || self.overlay != overlay_before || self.shown() != shown_before {'
                Replace = 'if self.overlay != overlay_before || self.shown() != shown_before {' }) }
    @{ Name = 'stale-age-not-redrawn'; File = 'src\app.rs'; Expect = 'Render is emitted iff the overlay, the feed status, the shown text or a chart load changed'
        Edits = @(@{ Find = '(self.status(), self.display(), self.tooltip())'
                Replace = '(self.status(), TickerDisplay::new(&self.status(), self.settings(), self.started_at), String::new())' }) }
    @{ Name = 'click-moves-ticker'; File = 'src\overlay.rs'; Expect = 'a click never moves the ticker'
        Edits = @(@{ Find = '(DragPhase::Pressed, PressTarget::Body) => next.expanded = !self.expanded,'
                Replace = '(DragPhase::Pressed, PressTarget::Body) => { next.expanded = !self.expanded; next.settings.position = Some(ticker_origin) }' }) }
    @{ Name = 'locked-check-dropped-from-pointer-move'; File = 'src\overlay.rs'; Expect = 'a locked press released over the body is a click, however far the pointer moved'
        Edits = @(@{ Find = 'if self.drag == DragPhase::Pressed && !self.settings.locked && distance >= Self::DRAG_THRESHOLD {'
                Replace = 'if self.drag == DragPhase::Pressed && distance >= Self::DRAG_THRESHOLD {' }) }
    @{ Name = 'locked-ticker-drags'; File = 'src\overlay.rs'; Expect = 'a locked ticker never drags'
        Edits = @(@{ Find = 'if self.drag == DragPhase::Pressed && !self.settings.locked && distance >= Self::DRAG_THRESHOLD {'
                Replace = 'if self.drag == DragPhase::Pressed && distance >= Self::DRAG_THRESHOLD {' },
            @{ Find = 'if !next.is_visible() || (next.settings.locked && next.drag == DragPhase::Dragging) {'
                Replace = 'if !next.is_visible() {' }) }
    @{ Name = 'hidden-overlay-keeps-drag'; File = 'src\overlay.rs'; Expect = 'a hidden overlay is never pressed or dragging'
        Edits = @(@{ Find = 'if !next.is_visible() || (next.settings.locked && next.drag == DragPhase::Dragging) {'
                Replace = 'if next.settings.locked && next.drag == DragPhase::Dragging {' }) }
    @{ Name = 'zoom-while-hidden'; File = 'src\overlay.rs'; Expect = 'only a zoom event on a visible chart changes the zoom, apart from a settings edit'
        Edits = @(@{ Find = 'if self.expanded && self.is_visible() {'; Replace = 'if self.expanded {' }) }
    @{ Name = 'zoom-while-collapsed'; File = 'src\overlay.rs'; Expect = 'only a zoom event on a visible chart changes the zoom, apart from a settings edit'
        Edits = @(@{ Find = 'if self.expanded && self.is_visible() {'; Replace = 'if self.is_visible() {' }) }
    @{ Name = 'zoom-in-steps-out'; File = 'src\market.rs'; Expect = 'a zoom event on a visible chart moves one rung its way, or stays at the end of the ladder'
        Edits = @(@{ Find = 'ZoomStep::In => self.index().checked_sub(1),'; Replace = 'ZoomStep::In => Some(self.index() + 1),' }) }
    @{ Name = 'zoom-drops-the-press'; File = 'src\overlay.rs'; Expect = 'a zoom event changes nothing but the zoom: never mode, timeframe, position, lock or drag'
        Edits = @(@{ Find = 'next.settings.zoom = self.settings.zoom.stepped(step);'
                Replace = 'next.settings.zoom = self.settings.zoom.stepped(step); next.drag = DragPhase::Idle;' }) }
    @{ Name = 'zoom-out-skips-a-rung'; File = 'src\market.rs'; Expect = 'on a visible chart away from the ends of the ladder, zooming in then out returns to the same state'
        Edits = @(@{ Find = 'ZoomStep::Out => Some(self.index() + 1),'; Replace = 'ZoomStep::Out => Some((self.index() + 2).min(Self::ALL.len() - 1)),' }) }
)

function Get-Counterexample([string]$Log, [string]$Invariant) {
    $lines = Get-Content $Log
    $start = [array]::IndexOf($lines, "invariant broken: $Invariant")
    if ($start -lt 0) { return @() }
    $block = @($lines[$start])
    for ($i = $start + 1; $i -lt $lines.Count -and $lines[$i] -match '^\s+(reached by:|start:|\d+\.)'; $i++) { $block += $lines[$i] }
    return $block
}

$failed = $false
foreach ($mutant in $mutants) {
    if ($Only -and $mutant.Name -ne $Only) { continue }
    $path = Join-Path $crate $mutant.File
    $original = [IO.File]::ReadAllText($path, $utf8)
    $mutated = $original
    $stale = $null
    foreach ($edit in $mutant.Edits) {
        $count = ([regex]::Matches($mutated, [regex]::Escape($edit.Find))).Count
        if ($count -ne 1) { $stale = "Find occurs $count times: $($edit.Find)"; break }
        $mutated = $mutated.Replace($edit.Find, $edit.Replace)
    }
    if ($stale) {
        Write-Output ('{0,-12} mutant {1}  ({2})' -f 'FAIL', $mutant.Name, "stale, $stale")
        $failed = $true
        continue
    }
    $log = Join-Path $logs "$($mutant.Name).log"
    try {
        [IO.File]::WriteAllText($path, $mutated, $utf8)
        Push-Location $workspace
        cmd /c "cargo test -q -p ticker-core --test model_check > `"$log`" 2>&1"
        $exit = $LASTEXITCODE
        Pop-Location
    } finally {
        [IO.File]::WriteAllText($path, $original, $utf8)
    }
    # Only rustc's errors: cargo also prints "error: test failed" when the model check catches the mutant.
    $compiled = -not (Select-String -Path $log -Pattern '^error(\[E\d+\]|: could not compile)' -Quiet)
    $example = Get-Counterexample $log $mutant.Expect
    $ok = $compiled -and $exit -ne 0 -and $example.Count -gt 0
    $detail = if ($ok) { "caught: $($mutant.Expect)" } elseif (-not $compiled) { "does not compile, $log" } else { "not caught, exit $exit, $log" }
    Write-Output ('{0,-12} mutant {1}  ({2})' -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $mutant.Name, $detail)
    $example | Select-Object -Skip 1 | ForEach-Object { "    | $_" }
    if (-not $ok) { $failed = $true }
}

if ($failed) { exit 1 }
exit 0
