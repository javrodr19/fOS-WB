//! VPN Location Picker UI
//!
//! Minimalist, immediate-mode location picker for the browser status bar.
//! Zero CPU usage when idle - only draws when visible/interacted with.
//!
//! # Design
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                     Browser Window                       │
//! │                                                          │
//! │                                                          │
//! │                                                          │
//! ├─────────────────────────────────────────────────────────┤
//! │ Status Bar                               [DE ▼] 🔒 25ms │
//! └─────────────────────────────────────────────────────────┘
//!                                               │
//!                                    ┌──────────┴──────────┐
//!                                    │  🇩🇪 Germany    ✓   │
//!                                    │  🇯🇵 Japan          │
//!                                    │  🇺🇸 USA            │
//!                                    │  🇰🇷 S. Korea       │
//!                                    │  🇷🇺 Russia         │
//!                                    │  🇬🇧 UK             │
//!                                    │─────────────────────│
//!                                    │  ⚡ Disconnect      │
//!                                    └─────────────────────┘
//! ```

use std::time::{Duration, Instant};

/// Country flag emoji for each region
pub const FLAG_DE: &str = "🇩🇪";
pub const FLAG_JP: &str = "🇯🇵";
pub const FLAG_US: &str = "🇺🇸";
pub const FLAG_KR: &str = "🇰🇷";
pub const FLAG_RU: &str = "🇷🇺";
pub const FLAG_UK: &str = "🇬🇧";

/// VPN connection state for UI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnUiState {
    /// Not connected to any VPN
    Disconnected,
    /// Connecting/handshaking (show spinner)
    Connecting,
    /// Connected and working
    Connected,
    /// Connection failed
    Failed,
    /// Switching regions (zero-leak pause active)
    Switching,
}

impl VpnUiState {
    /// Get status icon
    pub fn icon(&self) -> &'static str {
        match self {
            VpnUiState::Disconnected => "⚪",
            VpnUiState::Connecting | VpnUiState::Switching => "⏳",
            VpnUiState::Connected => "🔒",
            VpnUiState::Failed => "❌",
        }
    }

    /// Get status color (RGB)
    pub fn color(&self) -> [u8; 3] {
        match self {
            VpnUiState::Disconnected => [128, 128, 128], // Gray
            VpnUiState::Connecting | VpnUiState::Switching => [255, 200, 0], // Yellow
            VpnUiState::Connected => [0, 200, 100],      // Green
            VpnUiState::Failed => [255, 80, 80],         // Red
        }
    }
}

/// Region info for display
#[derive(Debug, Clone)]
pub struct RegionInfo {
    pub code: &'static str,
    pub flag: &'static str,
    pub name: &'static str,
    pub latency_ms: Option<u32>,
    pub healthy: bool,
}

impl RegionInfo {
    /// Create region info for DE
    pub fn germany(latency_ms: Option<u32>, healthy: bool) -> Self {
        Self { code: "DE", flag: FLAG_DE, name: "Germany", latency_ms, healthy }
    }

    /// Create region info for JP
    pub fn japan(latency_ms: Option<u32>, healthy: bool) -> Self {
        Self { code: "JP", flag: FLAG_JP, name: "Japan", latency_ms, healthy }
    }

    /// Create region info for US
    pub fn usa(latency_ms: Option<u32>, healthy: bool) -> Self {
        Self { code: "US", flag: FLAG_US, name: "USA", latency_ms, healthy }
    }

    /// Create region info for KR
    pub fn korea(latency_ms: Option<u32>, healthy: bool) -> Self {
        Self { code: "KR", flag: FLAG_KR, name: "S. Korea", latency_ms, healthy }
    }

    /// Create region info for RU
    pub fn russia(latency_ms: Option<u32>, healthy: bool) -> Self {
        Self { code: "RU", flag: FLAG_RU, name: "Russia", latency_ms, healthy }
    }

    /// Create region info for UK
    pub fn uk(latency_ms: Option<u32>, healthy: bool) -> Self {
        Self { code: "UK", flag: FLAG_UK, name: "UK", latency_ms, healthy }
    }

    /// Get all default regions
    pub fn all_defaults() -> Vec<Self> {
        vec![
            Self::germany(None, true),
            Self::japan(None, true),
            Self::usa(None, true),
            Self::korea(None, true),
            Self::russia(None, true),
            Self::uk(None, true),
        ]
    }
}

/// VPN indicator widget state
#[derive(Debug)]
pub struct VpnIndicator {
    /// Current VPN state
    pub state: VpnUiState,
    /// Active region (if connected)
    pub active_region: Option<RegionInfo>,
    /// Current latency display
    pub latency_ms: Option<u32>,
    /// Is dropdown open?
    pub dropdown_open: bool,
    /// Available regions
    pub regions: Vec<RegionInfo>,
    /// Hover index in dropdown (-1 = none)
    pub hover_index: i32,
    /// Spinner animation frame
    spinner_frame: u8,
    /// Last spinner update
    last_spinner: Instant,
    /// Needs redraw?
    pub dirty: bool,
}

impl VpnIndicator {
    /// Create a new VPN indicator
    pub fn new() -> Self {
        Self {
            state: VpnUiState::Disconnected,
            active_region: None,
            latency_ms: None,
            dropdown_open: false,
            regions: RegionInfo::all_defaults(),
            hover_index: -1,
            spinner_frame: 0,
            last_spinner: Instant::now(),
            dirty: true,
        }
    }

    /// Update state (call from VpnRegionManager status)
    pub fn set_state(&mut self, state: VpnUiState) {
        if self.state != state {
            self.state = state;
            self.dirty = true;
        }
    }

    /// Set active region
    pub fn set_active_region(&mut self, region: Option<RegionInfo>) {
        self.active_region = region;
        self.dirty = true;
    }

    /// Set latency
    pub fn set_latency(&mut self, ms: Option<u32>) {
        self.latency_ms = ms;
        self.dirty = true;
    }

    /// Toggle dropdown
    pub fn toggle_dropdown(&mut self) {
        self.dropdown_open = !self.dropdown_open;
        self.dirty = true;
    }

    /// Close dropdown
    pub fn close_dropdown(&mut self) {
        if self.dropdown_open {
            self.dropdown_open = false;
            self.dirty = true;
        }
    }

    /// Update hover (returns true if changed)
    pub fn set_hover(&mut self, index: i32) -> bool {
        if self.hover_index != index {
            self.hover_index = index;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Tick animation (call each frame when connecting)
    pub fn tick(&mut self) {
        if self.state == VpnUiState::Connecting || self.state == VpnUiState::Switching {
            if self.last_spinner.elapsed() > Duration::from_millis(200) {
                self.spinner_frame = (self.spinner_frame + 1) % 4;
                self.last_spinner = Instant::now();
                self.dirty = true;
            }
        }
    }

    /// Get spinner character
    pub fn spinner(&self) -> &'static str {
        match self.spinner_frame {
            0 => "◐",
            1 => "◓",
            2 => "◑",
            _ => "◒",
        }
    }

    /// Check if we need to redraw
    pub fn needs_redraw(&self) -> bool {
        self.dirty || self.dropdown_open || 
        self.state == VpnUiState::Connecting ||
        self.state == VpnUiState::Switching
    }

    /// Clear dirty flag
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }
}

impl Default for VpnIndicator {
    fn default() -> Self {
        Self::new()
    }
}

/// Actions from location picker
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocationAction {
    /// No action
    None,
    /// Toggle dropdown visibility
    ToggleDropdown,
    /// Select a region by code
    SelectRegion(String),
    /// Disconnect from VPN
    Disconnect,
}

/// Location picker widget
///
/// # Zero CPU When Idle
///
/// - Only redraws when `dirty` flag is set
/// - Dropdown hidden = no draw calls
/// - No animation unless connecting
pub struct LocationPicker {
    /// VPN indicator state
    pub indicator: VpnIndicator,
    /// Widget bounds (x, y, w, h)
    pub bounds: (f32, f32, f32, f32),
    /// Dropdown bounds
    pub dropdown_bounds: (f32, f32, f32, f32),
    /// Row height in dropdown
    pub row_height: f32,
}

impl LocationPicker {
    /// Create a new location picker
    /// 
    /// Position is typically bottom-right of status bar
    pub fn new(x: f32, y: f32) -> Self {
        let indicator_width = 100.0;
        let indicator_height = 24.0;
        let dropdown_width = 150.0;
        let row_height = 28.0;
        let num_regions = 6;
        let dropdown_height = row_height * (num_regions + 1) as f32 + 8.0; // +1 for disconnect

        Self {
            indicator: VpnIndicator::new(),
            bounds: (x, y, indicator_width, indicator_height),
            dropdown_bounds: (
                x + indicator_width - dropdown_width,
                y - dropdown_height,
                dropdown_width,
                dropdown_height,
            ),
            row_height,
        }
    }

    /// Handle click at position
    pub fn handle_click(&mut self, x: f32, y: f32) -> LocationAction {
        // Check indicator click
        if self.hit_test_indicator(x, y) {
            self.indicator.toggle_dropdown();
            return LocationAction::ToggleDropdown;
        }

        // Check dropdown clicks
        if self.indicator.dropdown_open {
            if let Some(action) = self.hit_test_dropdown(x, y) {
                self.indicator.close_dropdown();
                return action;
            }

            // Click outside closes dropdown
            self.indicator.close_dropdown();
        }

        LocationAction::None
    }

    /// Handle mouse move
    pub fn handle_move(&mut self, x: f32, y: f32) {
        if !self.indicator.dropdown_open {
            return;
        }

        let (dx, dy, dw, dh) = self.dropdown_bounds;
        
        if x >= dx && x <= dx + dw && y >= dy && y <= dy + dh {
            let row = ((y - dy - 4.0) / self.row_height) as i32;
            self.indicator.set_hover(row);
        } else {
            self.indicator.set_hover(-1);
        }
    }

    /// Check if point is in indicator
    fn hit_test_indicator(&self, x: f32, y: f32) -> bool {
        let (bx, by, bw, bh) = self.bounds;
        x >= bx && x <= bx + bw && y >= by && y <= by + bh
    }

    /// Check if point is in dropdown, return action
    fn hit_test_dropdown(&self, x: f32, y: f32) -> Option<LocationAction> {
        let (dx, dy, dw, dh) = self.dropdown_bounds;
        
        if x < dx || x > dx + dw || y < dy || y > dy + dh {
            return None;
        }

        let row = ((y - dy - 4.0) / self.row_height) as usize;
        
        if row < self.indicator.regions.len() {
            let region = &self.indicator.regions[row];
            Some(LocationAction::SelectRegion(region.code.to_string()))
        } else if row == self.indicator.regions.len() {
            Some(LocationAction::Disconnect)
        } else {
            None
        }
    }

    /// Generate text for status bar indicator
    pub fn indicator_text(&self) -> String {
        let code = self.indicator.active_region
            .as_ref()
            .map(|r| r.code)
            .unwrap_or("--");

        let icon = if self.indicator.state == VpnUiState::Connecting ||
                      self.indicator.state == VpnUiState::Switching {
            self.indicator.spinner()
        } else {
            self.indicator.state.icon()
        };

        let latency = self.indicator.latency_ms
            .map(|ms| format!("{}ms", ms))
            .unwrap_or_default();

        if latency.is_empty() {
            format!("[{}] {}", code, icon)
        } else {
            format!("[{}] {} {}", code, icon, latency)
        }
    }

    /// Generate draw commands for indicator
    ///
    /// Returns immediate-mode draw commands (rect, text, etc.)
    pub fn draw_indicator(&self) -> Vec<DrawCmd> {
        let mut cmds = Vec::new();
        let (x, y, w, h) = self.bounds;

        // Background
        cmds.push(DrawCmd::Rect {
            x, y, w, h,
            color: [40, 40, 45, 255],
            corner_radius: 4.0,
        });

        // Status text
        let text = self.indicator_text();
        let color = self.indicator.state.color();

        cmds.push(DrawCmd::Text {
            x: x + 8.0,
            y: y + h / 2.0 + 4.0,
            text,
            color: [color[0], color[1], color[2], 255],
            size: 12.0,
        });

        // Dropdown arrow
        cmds.push(DrawCmd::Text {
            x: x + w - 16.0,
            y: y + h / 2.0 + 4.0,
            text: if self.indicator.dropdown_open { "▲" } else { "▼" }.to_string(),
            color: [180, 180, 180, 255],
            size: 10.0,
        });

        cmds
    }

    /// Generate draw commands for dropdown (if open)
    pub fn draw_dropdown(&self) -> Vec<DrawCmd> {
        if !self.indicator.dropdown_open {
            return Vec::new();
        }

        let mut cmds = Vec::new();
        let (x, y, w, h) = self.dropdown_bounds;

        // Background with shadow
        cmds.push(DrawCmd::Rect {
            x: x + 2.0, y: y - 2.0, w, h,
            color: [0, 0, 0, 80],
            corner_radius: 6.0,
        });
        cmds.push(DrawCmd::Rect {
            x, y, w, h,
            color: [35, 35, 40, 250],
            corner_radius: 6.0,
        });

        // Region rows
        let mut row_y = y + 4.0;
        for (i, region) in self.indicator.regions.iter().enumerate() {
            let is_hover = self.indicator.hover_index == i as i32;
            let is_active = self.indicator.active_region
                .as_ref()
                .map(|r| r.code == region.code)
                .unwrap_or(false);

            // Hover highlight
            if is_hover {
                cmds.push(DrawCmd::Rect {
                    x: x + 4.0,
                    y: row_y,
                    w: w - 8.0,
                    h: self.row_height - 2.0,
                    color: [60, 60, 70, 255],
                    corner_radius: 4.0,
                });
            }

            // Flag + name
            let text = format!("{} {}", region.flag, region.name);
            let color = if region.healthy {
                [220, 220, 220, 255]
            } else {
                [150, 150, 150, 255]
            };

            cmds.push(DrawCmd::Text {
                x: x + 12.0,
                y: row_y + self.row_height / 2.0 + 4.0,
                text,
                color,
                size: 13.0,
            });

            // Checkmark for active
            if is_active {
                cmds.push(DrawCmd::Text {
                    x: x + w - 24.0,
                    y: row_y + self.row_height / 2.0 + 4.0,
                    text: "✓".to_string(),
                    color: [100, 200, 100, 255],
                    size: 14.0,
                });
            }

            // Latency
            if let Some(ms) = region.latency_ms {
                cmds.push(DrawCmd::Text {
                    x: x + w - 50.0,
                    y: row_y + self.row_height / 2.0 + 4.0,
                    text: format!("{}ms", ms),
                    color: [150, 150, 150, 255],
                    size: 10.0,
                });
            }

            row_y += self.row_height;
        }

        // Separator
        cmds.push(DrawCmd::Rect {
            x: x + 8.0,
            y: row_y,
            w: w - 16.0,
            h: 1.0,
            color: [70, 70, 80, 255],
            corner_radius: 0.0,
        });
        row_y += 4.0;

        // Disconnect button
        let is_disconnect_hover = self.indicator.hover_index == self.indicator.regions.len() as i32;
        if is_disconnect_hover {
            cmds.push(DrawCmd::Rect {
                x: x + 4.0,
                y: row_y,
                w: w - 8.0,
                h: self.row_height - 2.0,
                color: [80, 40, 40, 255],
                corner_radius: 4.0,
            });
        }

        cmds.push(DrawCmd::Text {
            x: x + 12.0,
            y: row_y + self.row_height / 2.0 + 4.0,
            text: "⚡ Disconnect".to_string(),
            color: [255, 120, 120, 255],
            size: 13.0,
        });

        cmds
    }

    /// Check if needs redraw
    pub fn needs_redraw(&self) -> bool {
        self.indicator.needs_redraw()
    }

    /// Mark as clean
    pub fn mark_clean(&mut self) {
        self.indicator.mark_clean();
    }
}

/// Immediate-mode draw command
#[derive(Debug, Clone)]
pub enum DrawCmd {
    /// Draw rectangle
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [u8; 4], // RGBA
        corner_radius: f32,
    },
    /// Draw text
    Text {
        x: f32,
        y: f32,
        text: String,
        color: [u8; 4],
        size: f32,
    },
}

/// Zero-leak network pause during region switch
///
/// When switching regions, all network traffic must be paused
/// for ~500ms to prevent the real IP from leaking during
/// the WireGuard handshake.
#[derive(Debug)]
pub struct ZeroLeakSwitch {
    /// Is switch in progress?
    active: bool,
    /// Switch started at
    started: Option<Instant>,
    /// Duration to pause (500ms default)
    pause_duration: Duration,
}

impl ZeroLeakSwitch {
    /// Create new zero-leak switch controller
    pub fn new() -> Self {
        Self {
            active: false,
            started: None,
            pause_duration: Duration::from_millis(500),
        }
    }

    /// Start the zero-leak pause
    pub fn start(&mut self) {
        self.active = true;
        self.started = Some(Instant::now());
    }

    /// Check if pause is complete
    pub fn is_complete(&self) -> bool {
        if !self.active {
            return true;
        }
        
        self.started
            .map(|s| s.elapsed() >= self.pause_duration)
            .unwrap_or(true)
    }

    /// Check if requests should be blocked
    pub fn should_block(&self) -> bool {
        self.active && !self.is_complete()
    }

    /// End the pause
    pub fn end(&mut self) {
        self.active = false;
        self.started = None;
    }

    /// Get remaining pause time
    pub fn remaining(&self) -> Duration {
        if !self.active {
            return Duration::ZERO;
        }
        
        self.started
            .map(|s| self.pause_duration.saturating_sub(s.elapsed()))
            .unwrap_or(Duration::ZERO)
    }
}

impl Default for ZeroLeakSwitch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vpn_ui_state() {
        assert_eq!(VpnUiState::Connected.icon(), "🔒");
        assert_eq!(VpnUiState::Disconnected.icon(), "⚪");
    }

    #[test]
    fn test_region_info() {
        let regions = RegionInfo::all_defaults();
        assert_eq!(regions.len(), 6);
        assert_eq!(regions[0].code, "DE");
    }

    #[test]
    fn test_vpn_indicator() {
        let mut indicator = VpnIndicator::new();
        
        assert!(!indicator.dropdown_open);
        indicator.toggle_dropdown();
        assert!(indicator.dropdown_open);
    }

    #[test]
    fn test_location_picker() {
        let mut picker = LocationPicker::new(500.0, 600.0);
        
        // Click on indicator
        let action = picker.handle_click(520.0, 610.0);
        assert_eq!(action, LocationAction::ToggleDropdown);
        assert!(picker.indicator.dropdown_open);
    }

    #[test]
    fn test_zero_leak_switch() {
        let mut zls = ZeroLeakSwitch::new();
        
        assert!(!zls.should_block());
        
        zls.start();
        assert!(zls.should_block());
        
        zls.end();
        assert!(!zls.should_block());
    }

    #[test]
    fn test_indicator_text() {
        let picker = LocationPicker::new(0.0, 0.0);
        let text = picker.indicator_text();
        
        // Should show disconnected state
        assert!(text.contains("--"));
    }
}
