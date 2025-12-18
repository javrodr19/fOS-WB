//! Browser Chrome - Minimal immediate-mode UI
//!
//! Implements the browser's UI elements:
//! - Tab strip
//! - Address bar
//! - Navigation buttons
//!
//! Uses immediate-mode rendering for minimal state and memory.

use crate::Color;
use tracing::debug;

/// Height of the browser chrome in pixels
pub const CHROME_HEIGHT: u32 = 72;

/// Height of the tab strip
pub const TAB_STRIP_HEIGHT: u32 = 36;

/// Height of the address bar
pub const ADDRESS_BAR_HEIGHT: u32 = 36;

/// Tab information
#[derive(Debug, Clone)]
pub struct TabInfo {
    /// Tab ID
    pub id: u64,
    /// Tab title
    pub title: String,
    /// Current URL
    pub url: String,
    /// Is this tab active?
    pub active: bool,
    /// Is tab loading?
    pub loading: bool,
    /// Favicon (RGBA data, 16x16)
    pub favicon: Option<Vec<u8>>,
}

impl TabInfo {
    pub fn new(id: u64, title: &str, url: &str) -> Self {
        Self {
            id,
            title: title.to_string(),
            url: url.to_string(),
            active: false,
            loading: false,
            favicon: None,
        }
    }
}

/// Chrome state for rendering
#[derive(Debug, Default)]
pub struct ChromeState {
    /// All tabs
    pub tabs: Vec<TabInfo>,
    /// Index of active tab
    pub active_tab: usize,
    /// Is address bar focused?
    pub address_bar_focused: bool,
    /// Text in address bar
    pub address_bar_text: String,
    /// Hover state
    pub hover_tab: Option<usize>,
    /// Can go back?
    pub can_go_back: bool,
    /// Can go forward?
    pub can_go_forward: bool,
}

impl ChromeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active_tab(&self) -> Option<&TabInfo> {
        self.tabs.get(self.active_tab)
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            // Deactivate old
            if let Some(old) = self.tabs.get_mut(self.active_tab) {
                old.active = false;
            }
            // Activate new
            self.active_tab = index;
            if let Some(new) = self.tabs.get_mut(index) {
                new.active = true;
                self.address_bar_text = new.url.clone();
            }
        }
    }

    pub fn add_tab(&mut self, title: &str, url: &str) -> u64 {
        let id = self.tabs.len() as u64 + 1;
        self.tabs.push(TabInfo::new(id, title, url));
        id
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() && self.tabs.len() > 1 {
            self.tabs.remove(index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
        }
    }
}

/// Browser chrome renderer (immediate-mode)
pub struct BrowserChrome {
    /// Current state
    state: ChromeState,
    /// Chrome width
    width: u32,
    /// Chrome height
    height: u32,
}

impl BrowserChrome {
    pub fn new(width: u32) -> Self {
        let mut chrome = Self {
            state: ChromeState::new(),
            width,
            height: CHROME_HEIGHT,
        };
        
        // Add default tab
        chrome.state.add_tab("New Tab", "about:blank");
        chrome.state.set_active_tab(0);
        
        chrome
    }

    /// Get state reference
    pub fn state(&self) -> &ChromeState {
        &self.state
    }

    /// Get mutable state
    pub fn state_mut(&mut self) -> &mut ChromeState {
        &mut self.state
    }

    /// Resize chrome
    pub fn resize(&mut self, width: u32) {
        self.width = width;
    }

    /// Get chrome height
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Generate draw commands for the chrome
    /// Returns a list of primitives to render
    pub fn draw(&self) -> Vec<DrawCommand> {
        let mut commands = Vec::new();

        // 1. Background
        commands.push(DrawCommand::Rect {
            x: 0.0,
            y: 0.0,
            width: self.width as f32,
            height: self.height as f32,
            color: Color::CHROME_BG,
        });

        // 2. Tab strip
        self.draw_tabs(&mut commands);

        // 3. Address bar
        self.draw_address_bar(&mut commands);

        commands
    }

    fn draw_tabs(&self, commands: &mut Vec<DrawCommand>) {
        let tab_count = self.state.tabs.len().max(1);
        let max_tab_width = 200.0f32;
        let min_tab_width = 50.0f32;
        
        // Calculate tab width based on available space
        let available_width = self.width as f32 - 100.0; // Reserve space for new tab button
        let tab_width = (available_width / tab_count as f32)
            .min(max_tab_width)
            .max(min_tab_width);

        for (i, tab) in self.state.tabs.iter().enumerate() {
            let x = i as f32 * tab_width;
            let y = 0.0;

            // Tab background
            let bg_color = if tab.active {
                Color::CHROME_TAB_ACTIVE
            } else if self.state.hover_tab == Some(i) {
                Color::CHROME_TAB
            } else {
                Color::CHROME_BG
            };

            commands.push(DrawCommand::RoundedRect {
                x,
                y: y + 4.0,
                width: tab_width - 2.0,
                height: TAB_STRIP_HEIGHT as f32 - 4.0,
                radius: 6.0,
                color: bg_color,
            });

            // Tab title
            let title = if tab.title.len() > 20 {
                format!("{}...", &tab.title[..17])
            } else {
                tab.title.clone()
            };

            commands.push(DrawCommand::Text {
                x: x + 12.0,
                y: y + 12.0,
                text: title,
                size: 12.0,
                color: Color::CHROME_TEXT,
            });

            // Close button (if hovered)
            if self.state.hover_tab == Some(i) {
                commands.push(DrawCommand::Text {
                    x: x + tab_width - 20.0,
                    y: y + 10.0,
                    text: "×".to_string(),
                    size: 16.0,
                    color: Color::CHROME_TEXT,
                });
            }
        }

        // New tab button
        let new_tab_x = self.state.tabs.len() as f32 * tab_width.min(max_tab_width);
        commands.push(DrawCommand::Text {
            x: new_tab_x + 10.0,
            y: 10.0,
            text: "+".to_string(),
            size: 18.0,
            color: Color::CHROME_TEXT,
        });
    }

    fn draw_address_bar(&self, commands: &mut Vec<DrawCommand>) {
        let y = TAB_STRIP_HEIGHT as f32;
        let padding = 8.0;
        let button_size = 28.0;

        // Navigation buttons
        let back_color = if self.state.can_go_back {
            Color::CHROME_TEXT
        } else {
            Color::rgba(0.5, 0.5, 0.5, 0.5)
        };

        commands.push(DrawCommand::Text {
            x: padding,
            y: y + 8.0,
            text: "←".to_string(),
            size: 18.0,
            color: back_color,
        });

        let forward_color = if self.state.can_go_forward {
            Color::CHROME_TEXT
        } else {
            Color::rgba(0.5, 0.5, 0.5, 0.5)
        };

        commands.push(DrawCommand::Text {
            x: padding + button_size,
            y: y + 8.0,
            text: "→".to_string(),
            size: 18.0,
            color: forward_color,
        });

        // Refresh button
        commands.push(DrawCommand::Text {
            x: padding + button_size * 2.0,
            y: y + 8.0,
            text: "↻".to_string(),
            size: 18.0,
            color: Color::CHROME_TEXT,
        });

        // Address bar background
        let bar_x = padding + button_size * 3.0 + 8.0;
        let bar_width = self.width as f32 - bar_x - padding - button_size;
        
        commands.push(DrawCommand::RoundedRect {
            x: bar_x,
            y: y + 4.0,
            width: bar_width,
            height: ADDRESS_BAR_HEIGHT as f32 - 8.0,
            radius: 14.0,
            color: Color::rgba(0.1, 0.1, 0.12, 1.0),
        });

        // URL text
        let url = if self.state.address_bar_focused {
            &self.state.address_bar_text
        } else if let Some(tab) = self.state.active_tab() {
            &tab.url
        } else {
            "about:blank"
        };

        let display_url = if url.len() > 60 {
            format!("{}...", &url[..57])
        } else {
            url.to_string()
        };

        commands.push(DrawCommand::Text {
            x: bar_x + 12.0,
            y: y + 10.0,
            text: display_url,
            size: 13.0,
            color: Color::CHROME_URL,
        });

        // Menu button
        commands.push(DrawCommand::Text {
            x: self.width as f32 - padding - 20.0,
            y: y + 8.0,
            text: "⋮".to_string(),
            size: 18.0,
            color: Color::CHROME_TEXT,
        });
    }

    /// Handle mouse click
    pub fn handle_click(&mut self, x: f32, y: f32) -> Option<ChromeAction> {
        // Check if in tab strip
        if y < TAB_STRIP_HEIGHT as f32 {
            return self.handle_tab_click(x, y);
        }

        // Check if in address bar
        if y < CHROME_HEIGHT as f32 {
            return self.handle_address_bar_click(x, y);
        }

        None
    }

    fn handle_tab_click(&mut self, x: f32, _y: f32) -> Option<ChromeAction> {
        let tab_count = self.state.tabs.len().max(1);
        let max_tab_width = 200.0f32;
        let available_width = self.width as f32 - 100.0;
        let tab_width = (available_width / tab_count as f32).min(max_tab_width);

        let tab_index = (x / tab_width) as usize;
        
        if tab_index < self.state.tabs.len() {
            self.state.set_active_tab(tab_index);
            return Some(ChromeAction::SwitchTab(tab_index));
        }

        // Check new tab button
        let new_tab_x = self.state.tabs.len() as f32 * tab_width;
        if x >= new_tab_x && x <= new_tab_x + 40.0 {
            let id = self.state.add_tab("New Tab", "about:blank");
            self.state.set_active_tab(self.state.tabs.len() - 1);
            return Some(ChromeAction::NewTab);
        }

        None
    }

    fn handle_address_bar_click(&mut self, x: f32, y: f32) -> Option<ChromeAction> {
        let button_size = 28.0;
        let padding = 8.0;

        // Back button
        if x < padding + button_size {
            return Some(ChromeAction::GoBack);
        }

        // Forward button
        if x < padding + button_size * 2.0 {
            return Some(ChromeAction::GoForward);
        }

        // Refresh button
        if x < padding + button_size * 3.0 {
            return Some(ChromeAction::Refresh);
        }

        // Address bar
        let bar_x = padding + button_size * 3.0 + 8.0;
        let bar_width = self.width as f32 - bar_x - padding - button_size;
        if x >= bar_x && x <= bar_x + bar_width {
            self.state.address_bar_focused = true;
            return Some(ChromeAction::FocusAddressBar);
        }

        None
    }

    /// Handle mouse move
    pub fn handle_mouse_move(&mut self, x: f32, y: f32) {
        if y < TAB_STRIP_HEIGHT as f32 {
            let tab_count = self.state.tabs.len().max(1);
            let max_tab_width = 200.0f32;
            let available_width = self.width as f32 - 100.0;
            let tab_width = (available_width / tab_count as f32).min(max_tab_width);
            
            let tab_index = (x / tab_width) as usize;
            self.state.hover_tab = if tab_index < self.state.tabs.len() {
                Some(tab_index)
            } else {
                None
            };
        } else {
            self.state.hover_tab = None;
        }
    }
}

/// Draw command for immediate-mode rendering
#[derive(Debug, Clone)]
pub enum DrawCommand {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    },
    RoundedRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: Color,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        color: Color,
    },
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        data: Vec<u8>,
    },
}

/// Actions triggered by chrome interaction
#[derive(Debug, Clone)]
pub enum ChromeAction {
    SwitchTab(usize),
    NewTab,
    CloseTab(usize),
    GoBack,
    GoForward,
    Refresh,
    Navigate(String),
    FocusAddressBar,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chrome_state() {
        let mut state = ChromeState::new();
        
        let id = state.add_tab("Test", "https://example.com");
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(id, 1);
        
        state.set_active_tab(0);
        assert!(state.tabs[0].active);
    }

    #[test]
    fn test_browser_chrome() {
        let chrome = BrowserChrome::new(1024);
        
        assert_eq!(chrome.height(), CHROME_HEIGHT);
        assert_eq!(chrome.state().tabs.len(), 1);
        
        let commands = chrome.draw();
        assert!(!commands.is_empty());
    }

    #[test]
    fn test_draw_commands() {
        let chrome = BrowserChrome::new(800);
        let commands = chrome.draw();
        
        // Should have at least: background, tabs, address bar
        assert!(commands.len() >= 3);
        
        // First should be background rect
        if let DrawCommand::Rect { width, height, .. } = &commands[0] {
            assert_eq!(*width, 800.0);
            assert_eq!(*height, CHROME_HEIGHT as f32);
        } else {
            panic!("Expected Rect");
        }
    }
}
