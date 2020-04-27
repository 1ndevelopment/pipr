use super::lineeditor::*;
use super::{
    command_list_window::CommandListState, key_select_menu::KeySelectMenu, main_window::AutocompleteState, pipr_config::*,
};
use crate::command_evaluation::*;
use crate::commandlist::CommandList;
use crossterm::event::{KeyCode, KeyModifiers};

pub const HELP_TEXT: &str = "\
F1         Show/hide help
F2         Toggle autoeval
F3         Toggle Paranoid history (fills up history in autoeval)
F4         Show/hide history
Ctrl+B     Show/hide bookmarks
F5         Open helpviewer
F6         Open outputviewer
Ctrl+S     Save bookmark
Alt+Return Newline
Ctrl+X     Clear Command
Ctrl+P     Previous in history
Ctrl+N     Next in history
Ctrl+V     Insert snippet (press corresponding key to choose)

disable a line by starting it with a #
this will simply exclude the line from the executed command.

Config file is in
~/.config/pipr/pipr.toml";

pub enum WindowState {
    Main,
    TextView(String, String),
    BookmarkList(CommandListState),
    HistoryList(CommandListState),
}

pub enum KeySelectMenuType {
    Snippets,
    OpenWordIn(String), // stores the word that should be opened in the selected help
    OpenOutputIn(String),
}

pub struct App {
    pub input_state: EditorState,
    pub command_output: String,
    pub command_error: String,
    pub autoeval_mode: bool,
    pub last_executed_cmd: String,
    pub paranoid_history_mode: bool,
    pub window_state: WindowState,
    pub bookmarks: CommandList,
    pub history: CommandList,
    pub history_idx: Option<usize>,
    pub execution_handler: CommandExecutionHandler,
    pub config: PiprConfig,
    pub should_quit: bool,
    pub opened_key_select_menu: Option<KeySelectMenu<KeySelectMenuType>>,
    pub raw_mode: bool,
    pub autocomplete_state: Option<AutocompleteState>,

    /// number from 0-4 showing an animation that shows some process being executed
    pub is_processing_state: Option<u8>,

    /// A (stdin, command) that should be executed in the main screen.
    /// this will be taken ( and thus reset ) and handled by the ui module.
    pub should_jump_to_other_cmd: Option<(Option<String>, std::process::Command)>,
}

impl App {
    pub fn new(
        execution_handler: CommandExecutionHandler,
        raw_mode: bool,
        config: PiprConfig,
        bookmarks: CommandList,
        history: CommandList,
    ) -> App {
        App {
            autocomplete_state: None,
            window_state: WindowState::Main,
            input_state: EditorState::new(),
            command_output: "".into(),
            command_error: "".into(),
            last_executed_cmd: "".into(),
            autoeval_mode: config.autoeval_mode_default,
            paranoid_history_mode: config.paranoid_history_mode_default,
            should_quit: false,
            is_processing_state: None,
            history_idx: None,
            opened_key_select_menu: None,
            should_jump_to_other_cmd: None,
            execution_handler,
            raw_mode,
            config,
            bookmarks,
            history,
        }
    }

    pub fn on_cmd_output(&mut self, process_result: CmdOutput) {
        self.is_processing_state = None;
        match process_result {
            CmdOutput::Ok(stdout) => {
                if self.paranoid_history_mode {
                    self.history.push(self.input_state.content_to_commandentry());
                }
                self.command_output = stdout;
                self.command_error = String::new();
            }
            CmdOutput::NotOk(stderr) => self.command_error = stderr,
        }
    }

    pub fn set_should_quit(&mut self) {
        self.should_quit = true;
        self.history.push(self.input_state.content_to_commandentry());
    }

    pub async fn execute_content(&mut self) {
        let command = self.input_state.content_lines();
        let command = command
            .iter()
            .filter(|line| !line.starts_with('#'))
            .map(|x| x.to_owned())
            .collect::<Vec<String>>();
        let command = if self.raw_mode {
            command.join("\n")
        } else {
            command.join(" ")
        };
        self.execution_handler.execute(&command).await;
        self.is_processing_state = Some(0);
        self.last_executed_cmd = self.input_state.content_str();
    }

    pub async fn on_tui_event(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let control_pressed = modifiers.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::F(1) => match self.window_state {
                WindowState::TextView(_, _) => self.window_state = WindowState::Main,
                _ => self.window_state = WindowState::TextView("Help".to_string(), HELP_TEXT.to_string()),
            },
            KeyCode::Char('b') if control_pressed => match self.window_state {
                WindowState::BookmarkList(_) => {
                    self.window_state = WindowState::Main;
                }
                _ => {
                    self.history.push(self.input_state.content_to_commandentry());

                    let entries = self.bookmarks.entries.clone();
                    self.window_state = WindowState::BookmarkList(CommandListState::new(entries, None));
                }
            },
            KeyCode::F(4) => match self.window_state {
                WindowState::HistoryList(_) => {
                    self.window_state = WindowState::Main;
                }
                _ => {
                    self.history.push(self.input_state.content_to_commandentry());

                    let entries = self.history.entries.clone();
                    self.window_state = WindowState::HistoryList(CommandListState::new(entries, self.history_idx));
                }
            },
            _ => self.handle_window_specific_event(code, modifiers).await,
        }
    }

    pub async fn handle_window_specific_event(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        let window_state = &mut self.window_state;
        match window_state {
            WindowState::Main => self.handle_main_window_tui_event(code, modifiers).await,

            WindowState::TextView(_, _) => self.window_state = WindowState::Main,
            WindowState::BookmarkList(state) => match code {
                KeyCode::Esc => {
                    self.bookmarks.entries = state.list.clone();
                    self.window_state = WindowState::Main;
                }
                KeyCode::Enter => {
                    if let Some(entry) = state.selected_entry() {
                        self.input_state.load_commandentry(entry);
                    }
                    self.bookmarks.entries = state.list.clone();
                    self.window_state = WindowState::Main;
                }
                _ => state.apply_event(code),
            },
            WindowState::HistoryList(state) => match code {
                KeyCode::Esc => {
                    self.history.entries = state.list.clone();
                    self.window_state = WindowState::Main;
                }
                KeyCode::Enter => {
                    if let Some(entry) = state.selected_idx.and_then(|idx| state.list.get(idx)) {
                        self.input_state.load_commandentry(entry);
                    }
                    self.history.entries = state.list.clone();
                    self.history_idx = state.selected_idx;
                    self.window_state = WindowState::Main;
                }
                _ => state.apply_event(code),
            },
        }
    }

    pub fn on_tick(&mut self) {
        self.is_processing_state = self.is_processing_state.map(|x| (x + 1) % 6)
    }
}
