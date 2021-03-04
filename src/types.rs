use crate::schema::{ParsedValue, SCHEMA};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use wasm_bindgen::JsCast;
use yew::{
    html,
    prelude::*,
    services::{storage::Area, StorageService},
    web_sys::HtmlElement,
    Component, ComponentLink, FocusEvent, Html, InputData, KeyboardEvent, ShouldRender,
};

pub type Ref = String;

pub const INVALID_REF: &str = "-";

pub fn new_ref() -> Ref {
    uuid::Uuid::new_v4().to_hyphenated().to_string()
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Selector {
    pub field: String,
    pub index: usize,
}

pub type Path = VecDeque<Selector>;

pub fn append(path: &Path, selector: Selector) -> Path {
    let mut new_path = path.clone();
    new_path.push_back(selector);
    new_path
}

#[derive(Debug, PartialEq, Clone)]
pub enum Mode {
    Normal,
    Edit,
}

pub struct Model {
    pub file: File,
    pub store: StorageService,
    pub cursor: Path,
    pub hover: Path,
    pub mode: Mode,

    pub link: ComponentLink<Self>,

    pub raw_command: String,
    pub parsed_commands: Vec<ParsedValue>,
    pub selected_command_index: usize,
}

impl Model {
    pub fn lookup(&self, reference: &Ref) -> Option<&Node> {
        self.file.lookup(reference)
    }
    pub fn lookup_path(&self, reference: &Ref, relative_path: Path) -> Option<Ref> {
        let mut path = relative_path;
        match path.pop_front() {
            Some(head) => {
                let base = self.lookup(reference).unwrap();
                let children = &base.children.get(&head.field).cloned().unwrap_or_default();
                let r = children
                    .get(head.index)
                    .cloned()
                    .unwrap_or(INVALID_REF.to_string());
                self.lookup_path(&r, path)
            }
            None => Some(reference.clone()),
        }
    }

    pub fn parent_ref(&self) -> Option<Ref> {
        let mut parent_cursor = self.cursor.clone();
        parent_cursor.pop_back().unwrap();
        self.lookup_path(&self.file.root, parent_cursor)
    }

    pub fn current_ref(&self) -> Option<Ref> {
        self.lookup_path(&self.file.root, self.cursor.clone())
    }

    fn parse_commands(&mut self) -> Vec<ParsedValue> {
        self.current_field()
            .map(|field| {
                field
                    .kind
                    .iter()
                    .filter_map(|kind| SCHEMA.get_kind(kind))
                    .flat_map(|kind| {
                        kind.parse(&self.raw_command)
                            .into_iter()
                            // TODO: Different matching logic (e.g. fuzzy).
                            .filter(|v| match &v.value {
                                Ok(v) => v.starts_with(&self.raw_command),
                                Err(_) => true,
                            })
                    })
                    .collect::<Vec<_>>()
                // TODO: Ranking.
            })
            .unwrap_or_default()
    }

    fn prev(&mut self) {
        let flattened_paths = self.flatten_paths(&self.file.root, Path::new());
        log::info!("paths: {:?}", flattened_paths);
        let current_path_index = flattened_paths.iter().position(|x| *x == self.cursor);
        log::info!("current: {:?}", current_path_index);
        if let Some(current_path_index) = current_path_index {
            if current_path_index > 0 {
                if let Some(path) = flattened_paths.get(current_path_index - 1) {
                    self.cursor = path.clone();
                }
            }
        }
    }

    fn next(&mut self) {
        let flattened_paths = self.flatten_paths(&self.file.root, Path::new());
        log::info!("paths: {:?}", flattened_paths);
        let current_path_index = flattened_paths.iter().position(|x| *x == self.cursor);
        log::info!("current: {:?}", current_path_index);
        if let Some(current_path_index) = current_path_index {
            if let Some(path) = flattened_paths.get(current_path_index + 1) {
                self.cursor = path.clone();
            }
        } else {
            if let Some(path) = flattened_paths.get(0) {
                self.cursor = path.clone();
            }
        }
    }

    fn set_node(&mut self, node: Node) {
        let mut node = node;

        let current_ref = self.current_ref();
        let node_kind = SCHEMA.get_kind(&node.kind);

        if let (Some(current_ref), Some(inner_field)) =
            (current_ref, node_kind.and_then(|k| k.inner()))
        {
            if current_ref != INVALID_REF {
                node.children
                    .insert(inner_field.to_string(), vec![current_ref.clone()]);
            }
        };

        let new_ref = self.file.add_node(node);

        let selector = self.cursor.back().unwrap().clone();
        let parent_ref = self.parent_ref().unwrap();
        log::info!("parent ref: {:?}", parent_ref);
        let mut parent = self.file.lookup(&parent_ref).unwrap().clone();
        log::info!("parent: {:?}", parent);

        // If the field does not exist, create a default one.
        let children = parent.children.entry(selector.field).or_default();
        match children.get_mut(selector.index) {
            Some(c) => *c = new_ref,
            None => children.push(new_ref),
        };
        self.file.replace_node(&parent_ref, parent);
    }

    fn replace_node(&mut self, node: Node) {
        if let Some(current_ref) = self.current_ref() {
            if current_ref != INVALID_REF {
                self.file.replace_node(&current_ref, node);
            } else {
                let new_ref = self.file.add_node(node);
                let selector = self.cursor.back().unwrap().clone();
                let parent_ref = self.parent_ref().unwrap();
                log::info!("parent ref: {:?}", parent_ref);
                let mut parent = self.file.lookup(&parent_ref).unwrap().clone();
                log::info!("parent: {:?}", parent);
                // If the field does not exist, create a default one.
                let children = parent.children.entry(selector.field).or_default();
                match children.get_mut(selector.index) {
                    Some(c) => *c = new_ref,
                    None => children.push(new_ref),
                };
                self.file.replace_node(&parent_ref, parent);
            }
        }
    }

    pub fn focus_command_line(&self) {
        yew::utils::document()
            .query_selector("#command-line")
            .unwrap()
            .unwrap()
            .dyn_into::<HtmlElement>()
            .unwrap()
            .focus()
            .unwrap();
    }

    pub fn scroll_into_view(&self, selector: &str) {
        if let Some(element) = yew::utils::document().query_selector(selector).unwrap() {
            element
                .dyn_into::<HtmlElement>()
                .unwrap()
                .scroll_into_view_with_bool(false)
        }
    }
}

#[derive(Clone)]
pub enum Msg {
    Noop,

    Select(Path),
    Hover(Path),

    Store,
    Load,

    Prev,
    Next,
    Parent,

    AddItem,
    DeleteItem,

    SetMode(Mode),

    SetCommand(String),
    NextCommand,
    PrevCommand,
    EnterCommand,
    EscapeCommand,

    ReplaceCurrentNode(Node),
    CommandKey(KeyboardEvent),
}

#[derive(Serialize, Deserialize)]
pub struct File {
    pub nodes: HashMap<Ref, Node>,
    pub root: Ref,
    pub log: Vec<(Ref, Node)>,
}

impl File {
    pub fn lookup(&self, reference: &Ref) -> Option<&Node> {
        self.nodes.get(reference)
    }

    // fn lookup_mut(&mut self, reference: &Ref) -> Option<&mut Node> {
    //     self.nodes.get_mut(reference)
    // }

    fn add_node(&mut self, node: Node) -> Ref {
        let reference = new_ref();
        self.replace_node(&reference, node);
        reference
    }

    fn replace_node(&mut self, reference: &Ref, node: Node) {
        self.log.push((reference.clone(), node.clone()));
        self.nodes.insert(reference.clone(), node);
    }
}

// TODO: Navigate to children directly, but use :var to navigate to variables, otherwise skip them
// when navigating.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Node {
    pub kind: String,
    pub value: String,
    pub children: HashMap<String, Vec<Ref>>,
}

fn display_selector(selector: &Selector) -> Vec<Html> {
    let mut segments = Vec::new();
    segments.push(html! {
        <span>
          { selector.field.clone() }
              { format!("[{}]", selector.index) }
        </span>
    });
    segments.push(html! {
        <span>
          { format!("[{}]", selector.index) }
        </span>
    });
    segments
}

fn display_cursor(cursor: &Path) -> Html {
    let segments = cursor
        .iter()
        .flat_map(|s| display_selector(s))
        .intersperse(html! { <span>{ ">" }</span>});
    html! {
        <div>{ for segments }</div>
    }
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn view(&self) -> Html {
        let onkeypress = self
            .link
            .callback(move |e: KeyboardEvent| Msg::CommandKey(e));
        let oninput = self
            .link
            .callback(move |e: InputData| Msg::SetCommand(e.value));
        let onblur = self.link.callback(move |e: FocusEvent| Msg::Noop);
        let values = self.parsed_commands.iter().enumerate().map(|(i, parsed_value)| {
            // let callback = self.link.callback(move |_| Msg::ReplaceCurrentNode(v));
            let mut classes = vec![
                "border",
                "border-gray-200",
                "flex",
                "group",
                "block",
                "rounded-lg",
                "p-2",
                "m-2",
            ];
            if self.selected_command_index == i {
                classes.push("bg-blue-300");
            } else {
                classes.push("bg-gray-100");
            }
            let value = parsed_value.value.clone().ok().unwrap_or_default();
            let (prefix, suffix) = match value.strip_prefix(&self.raw_command) {
                Some(suffix) => (self.raw_command.clone(), suffix.to_string()),
                None => ("".to_string(), value.clone()),
            };
            let kinds = parsed_value.kind_hierarchy.iter().map(|k| {
                html!{
                    <span class="kind bg-blue-200 p-1 px-2 rounded-md text-sm font-mono">
                        <svg class="w-4 h-4 mr-1 inline" fill="none" stroke="currentColor" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4"></path></svg>
                        { k }
                    </span>
                }
            });
            html! {
                <div
                //   onclick=callback
                // XXX
                  class=classes.join(" ")>
                  <i class="gg-attachment m-5"></i>
                  <div>
                    <div class="font-mono">
                        <span class="text-green-400 font-bold underline">
                            { prefix }
                        </span>
                        <span class="">
                            { suffix }
                        </span>
                    </div>
                    <div>
                        { for kinds }
                    </div>
                  </div>
                </div>
            }
        });
        // self.scroll_into_view(&format!(
        //     "#values div:nth-child({})",
        //     self.selected_command_index + 1,
        // ));
        let onmouseover = self.link.callback(move |e: MouseEvent| {
            e.stop_propagation();
            Msg::Hover(vec![].into())
        });
        html! {
            <div onkeydown=onkeypress onmouseover=onmouseover>
                <div>{ "LINC" }</div>
                <div>{ "left / right arrow keys (when command line is empty): move between existing nodes" }</div>
                <div>{ "up / down arrow keys: select alternative completion result" }</div>
                <div>{ "start typing in command line to filter available completion results" }</div>
                <div class="">
                    <div class="column">{ self.view_file(&self.file) }</div>

                    <div>{ "Mode: " }{ format!("{:?}", self.mode) }</div>
                    <div>{ self.view_actions() }</div>
                    <div class="relative">
                        <span class="inline-block absolute w-2 h-20 left-5 top-5">
                            <i class="gg-terminal"></i>
                        </span>
                        <input
                            id="command-line"
                            class="p-2 m-2 border border-blue-500 bg-blue-100 font-mono rounded-lg pl-10 w-full"
                            oninput=oninput
                            onblur=onblur
                            disabled={ self.mode == Mode::Normal }
                            value=self.raw_command
                        />
                        <div class="h-60">
                            // <CommandLine values=allowed_kinds on_change=callback base_value=self.command.clone() state=state />
                            <div id="values" class="overflow-y-scroll h-60 gap-4">
                                { for values }
                            </div>
                        </div>
                    </div>
                    <div class="h-40">
                        <div class="column">
                            <div>{ display_cursor(&self.cursor) }</div>
                            <div>{ format!("Ref: {:?}", self.lookup_path(&self.file.root, self.cursor.clone())) }</div>
                        </div>
                        <div class="column">{ self.view_file_json(&self.file) }</div>
                    </div>
                </div>
            </div>
        }
    }

    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        Model {
            store: StorageService::new(Area::Local).expect("could not create storage service"),
            raw_command: "".to_string(),
            parsed_commands: vec![],
            selected_command_index: 0,
            file: super::initial::initial(),
            mode: Mode::Normal,
            cursor: vec![Selector {
                field: "items".to_string(),
                index: 0,
            }]
            .into(),
            hover: vec![].into(),
            link,
        }
    }

    fn change(&mut self, _props: ()) -> ShouldRender {
        self.focus_command_line();
        false
    }

    fn rendered(&mut self, _first_render: bool) {
        self.focus_command_line();
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        fn update_from_selected(model: &mut Model) {
            let current_node = model.current_ref().and_then(|r| model.lookup(&r)).cloned();
            let current_kind = current_node.clone().map(|n| n.kind.clone());
            model.raw_command = current_node.map(|n| n.value.clone()).unwrap_or_default();
            model.parsed_commands = model.parse_commands();
            model.selected_command_index = model
                .parsed_commands
                .iter()
                .position(|parsed_value| {
                    parsed_value.kind_hierarchy.last() == current_kind.as_ref()
                })
                .unwrap_or(0);
        }

        const KEY: &str = "linc_file";
        match msg {
            Msg::Noop => {}
            Msg::Select(path) => {
                self.cursor = path;
                update_from_selected(self);
            }
            Msg::Hover(path) => {
                self.hover = path;
            }
            // TODO: sibling vs inner
            Msg::Prev => {
                self.prev();
                update_from_selected(self);
            }
            // Preorder tree traversal.
            Msg::Next => {
                self.next();
                update_from_selected(self);
            }
            Msg::Parent => {
                self.cursor.pop_back();
                update_from_selected(self);
            }
            Msg::Store => {
                self.store.store(KEY, yew::format::Json(&self.file));
            }
            Msg::Load => {
                if let yew::format::Json(Ok(file)) = self.store.restore(KEY) {
                    self.file = file;
                }
            }
            Msg::SetMode(mode) => {
                self.mode = mode;
            }
            Msg::ReplaceCurrentNode(n) => {
                self.file.replace_node(&self.current_ref().unwrap(), n);
                self.parsed_commands = self.parse_commands();
                self.selected_command_index = 0;
            }
            Msg::AddItem => {
                let selector = self.cursor.back().unwrap().clone();
                let parent_ref = self.parent_ref().unwrap();
                let new_ref = self.file.add_node(Node {
                    kind: "invalid".to_string(),
                    value: "invalid".to_string(),
                    children: HashMap::new(),
                });
                let mut parent = self.file.lookup(&parent_ref).unwrap().clone();
                // If the field does not exist, create a default one.
                let children = parent.children.entry(selector.field).or_default();
                let new_index = selector.index + 1;
                children.insert(new_index, new_ref);
                self.file.replace_node(&parent_ref, parent);
                // Select newly created element.
                self.cursor.back_mut().unwrap().index = new_index;
            }
            Msg::DeleteItem => {
                let selector = self.cursor.back().unwrap().clone();
                let parent_ref = self.parent_ref().unwrap();
                let mut parent = self.file.lookup(&parent_ref).unwrap().clone();
                // If the field does not exist, create a default one.
                let children = parent.children.entry(selector.field).or_default();
                children.remove(selector.index);
                self.file.replace_node(&parent_ref, parent);
            }
            Msg::SetCommand(v) => {
                self.raw_command = v;
                self.parsed_commands = self.parse_commands();
                self.selected_command_index = 0;
            }
            Msg::NextCommand => {
                if self.selected_command_index < (self.parsed_commands.len() - 1) {
                    self.selected_command_index += 1;
                }
            }
            Msg::PrevCommand => {
                if self.selected_command_index > 0 {
                    self.selected_command_index -= 1;
                }
            }
            Msg::EnterCommand => {
                match self
                    .parsed_commands
                    .get(self.selected_command_index)
                    .clone()
                {
                    Some(parsed_value) => {
                        // Replace current node.
                        // self.file
                        //     .nodes
                        //     .insert(self.current_ref().unwrap(), node.clone());
                        match parsed_value.to_node() {
                            Some(node) => {
                                // self.set_node(node.clone());
                                self.replace_node(node.clone());
                                self.next();
                                self.raw_command = "".to_string();
                                self.parsed_commands = self.parse_commands();
                                self.selected_command_index = 0;
                            }
                            None => {}
                        }
                    }
                    None => log::info!("invalid command"),
                }
            }
            Msg::EscapeCommand => {
                self.raw_command = "".to_string();
                self.parsed_commands = self.parse_commands();
                self.selected_command_index = 0;
                self.mode = Mode::Normal;
            }
            Msg::CommandKey(v) => {
                log::info!("key: {}", v.key());
                // See https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/code
                match v.key().as_ref() {
                    "Enter" => self.link.send_message(Msg::EnterCommand),
                    "Escape" => self.link.send_message(Msg::EscapeCommand),
                    "ArrowUp" => self.link.send_message(Msg::PrevCommand),
                    "ArrowDown" => self.link.send_message(Msg::NextCommand),

                    /*
                    "ArrowRight" if self.raw_command.is_empty() => {
                        self.next();
                        self.parsed_commands = self.parse_commands();
                        self.selected_command_index = 0;
                        update_from_selected(self);
                    }
                    "ArrowLeft" if self.raw_command.is_empty() => {
                        self.prev();
                        self.parsed_commands = self.parse_commands();
                        self.selected_command_index = 0;
                        update_from_selected(self);
                    }
                    */
                    _ => {}
                }
            }
        };
        self.focus_command_line();
        true
    }
}

pub struct Action {
    pub image: Option<String>,
    pub text: String,
    pub msg: Msg,
}
