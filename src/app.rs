use std::collections::{HashMap, HashSet};
use std::path::Path;

use imnodes::{InputPinId, LinkId, NodeId, OutputPinId};

use crate::id_gen::GeneratesId;

use crate::message::{Message, MessageQueue, SendData, TaggedMessage};
use crate::nodes::specialization::{LinkEvent, NodeSpecialization};
use crate::pins::Pin;

#[derive(Debug, Clone)]
pub struct Link {
    pub id: LinkId,
    pub input_pin_id: InputPinId,
    pub output_pin_id: OutputPinId,
}

impl Link {
    pub fn new(input_pin_id: InputPinId, output_pin_id: OutputPinId) -> Self {
        Self {
            id: LinkId::generate(),
            input_pin_id,
            output_pin_id,
        }
    }
}

#[derive(Default)]
pub struct App {
    pub(crate) nodes: HashMap<NodeId, Box<dyn NodeSpecialization>>,
    pub(crate) input_pins: HashMap<InputPinId, NodeId>,
    pub(crate) output_pins: HashMap<OutputPinId, NodeId>,
    pub(crate) links: Vec<Link>,
    pub state: Option<AppState>,
    pub messages: MessageQueue,
    pub received_messages: HashMap<NodeId, HashSet<usize>>,
}

pub enum AppState {
    AddingNode { name: String, index: usize },
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Box<dyn NodeSpecialization>) {
        let node_id = node.id();
        for input in node.inputs().unwrap_or_default() {
            self.input_pins.insert(*input.id(), node_id);
        }
        for output in node.outputs().unwrap_or_default() {
            self.output_pins.insert(*output.id(), node_id);
        }
        self.nodes.insert(node_id, node);
    }

    pub fn get_node(&self, id: NodeId) -> Option<&dyn NodeSpecialization> {
        self.nodes.get(&id).map(Box::as_ref)
    }

    pub fn get_link(&self, input_id: &InputPinId) -> Option<&Link> {
        self.links
            .iter()
            .find(|link| link.input_pin_id == *input_id)
    }

    pub fn remove_node(&mut self, id: &NodeId) -> Option<Box<dyn NodeSpecialization>> {
        let node = self.nodes.remove(id)?;
        for input in node.inputs().unwrap_or_default() {
            self.input_pins.remove(input.id());
        }
        for output in node.outputs().unwrap_or_default() {
            self.output_pins.remove(output.id());
        }
        Some(node)
    }

    fn handle_message(&mut self, tagged: TaggedMessage) -> Option<Vec<Message>> {
        match tagged.message {
            Message::SendData(SendData {
                data,
                from_output: _,
                to_input,
            }) => {
                let node_id = self.input_pins.get_mut(&to_input).unwrap();
                let node = self.nodes.get_mut(node_id).unwrap();
                let received_msgs = self.received_messages.entry(*node_id).or_default();
                if received_msgs.contains(&tagged.tag) {
                    return None;
                }
                received_msgs.insert(tagged.tag);
                node.notify(LinkEvent::Push {
                    from_pin_id: to_input,
                    payload: data.clone(),
                })
            }
            Message::AddLink(link) => {
                if self.get_link(&link.input_pin_id).is_some() {
                    return None;
                }
                try {
                    let Link {
                        input_pin_id,
                        output_pin_id,
                        ..
                    } = &link;
                    let node_ids = [
                        self.input_pins.get(input_pin_id)?,
                        self.output_pins.get(output_pin_id)?,
                    ];
                    let [input_node, output_node] = self.nodes.get_many_mut(node_ids)?;
                    if !input_node.should_link(input_pin_id) {
                        // Poor man's early return
                        None?
                    }
                    input_node
                        .get_input_mut(input_pin_id)?
                        .link_to(output_pin_id);
                    output_node
                        .get_output_mut(output_pin_id)?
                        .link_to(input_pin_id);
                    self.links.push(link);
                    output_node.broadcast_data()
                }
            }
            Message::RemoveLink(link) => {
                let Link {
                    input_pin_id,
                    output_pin_id,
                    ..
                } = &link;
                let node_ids = [
                    self.input_pins.get(input_pin_id)?,
                    self.output_pins.get(output_pin_id)?,
                ];
                let [input_node, output_node] = self.nodes.get_many_mut(node_ids)?;
                input_node
                    .get_input_mut(input_pin_id)?
                    .unlink(output_pin_id);
                output_node
                    .get_output_mut(output_pin_id)?
                    .unlink(input_pin_id);
                input_node.notify(LinkEvent::Pop(*input_pin_id))
            }
        }
    }

    pub fn add_link(&mut self, start_pin: OutputPinId, end_pin: InputPinId) {
        self.messages
            .push(Message::AddLink(Link::new(end_pin, start_pin)));
    }

    pub fn remove_link(&mut self, link_id: LinkId) {
        let Some(index) = self.links.iter().position(|link| link.id == link_id) else {
            return;
        };
        let link = self.links.swap_remove(index);
        self.messages.push(Message::RemoveLink(link));
    }

    pub fn update(&mut self) {
        let mut new_messages = MessageQueue::with_tag(self.messages.current_tag());
        for tagged in std::mem::take(&mut self.messages) {
            let tag = tagged.tag;
            let newmsgs = self.handle_message(tagged);
            for newmsg in newmsgs.unwrap_or_default() {
                new_messages.push_tagged(newmsg, tag);
            }
        }
        self.messages = new_messages;
    }

    pub fn save_sate(&self) {
        let mut arguments = Vec::new();
        let mut equations = odeir::Map::new();

        for node in self.nodes.values() {
            let arg = node.to_equation(self);
            arguments.push(arg);
        }

        let json = odeir::Json {
            metadata: odeir::Metadata {
                name: "TODO".to_string(),
                model_metadata: odeir::ModelMetadata::ODE(odeir::models::ode::Metadata {
                    start_time: 0.0,
                    delta_time: 0.0,
                    end_time: 0.0,
                }),
                positions: odeir::Map::new(),
            },
            arguments,
            equations,
        };

        println!("{}", serde_json::to_string_pretty(&json).unwrap());
        // let folder = folder.as_ref();
        // let model = self.as_model();
        /* let model = odeir::model_into_json(&model);
        let ui: &imnodes::EditorScope = todo!();
        std::fs::write(folder.join("model.json"), model) */
    }

    // pub fn load_state(&self, )
}

/* impl App {
    pub fn as_model(&self) -> odeir::Model {
        let nodes = self
            .nodes
            .iter()
            .filter_map(|(id, n)| Some((*id as u32, n.as_odeir_node()?)))
            .collect();
        let constants = self
            .nodes
            .iter()
            .filter_map(|(_, n)| Some(n.as_odeir_constant()?))
            .collect();
        odeir::Model {
            nodes,
            constants,
            // !TODO: implement medatada
            meta_data: odeir::MetaData::default(),
        }
    }
} */
