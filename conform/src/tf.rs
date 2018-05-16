#![allow(dead_code)]

use std::{fs, path};

use tensorflow::Graph;
use tensorflow::Session;
use tensorflow::StepWithGraph;
use tensorflow::OutputToken;
use tensorflow::DataType;
use tensorflow::Tensor;

use tfdeploy::Matrix;

use ndarray::ArrayD;

use std::collections::HashMap;
use std::collections::HashSet;

use ::Result;

pub struct Tensorflow {
    session: Session,
    graph: Graph,
}

pub fn for_path<P: AsRef<path::Path>>(p: P) -> Result<Tensorflow> {
    use std::io::Read;
    let mut model = vec![];
    fs::File::open(p)?.read_to_end(&mut model)?;
    for_slice(&*model)
}

pub fn for_slice(buf: &[u8]) -> Result<Tensorflow> {
    let mut graph = Graph::new();
    graph.import_graph_def(buf, &::tensorflow::ImportGraphDefOptions::new())?;
    let session = Session::new(&::tensorflow::SessionOptions::new(), &graph)?;
    Ok(Tensorflow { session, graph })
}

enum TensorHolder {
    F64(Tensor<f64>),
    F32(Tensor<f32>),
    I32(Tensor<i32>),
    U8(Tensor<u8>),
    I8(Tensor<i8>),
    String(Tensor<i8>),
}

impl TensorHolder {
    fn to_tensor<T: ::tensorflow::TensorType + Copy>(m: ArrayD<T>) -> Tensor<T> {
        let dims: Vec<u64> = m.shape().iter().map(|d| *d as _).collect();
        let mut tensor = Tensor::<T>::new(&*dims);
        tensor.copy_from_slice(m.as_slice().unwrap());
        tensor
    }
}

impl From<Matrix> for TensorHolder {
    fn from(m: Matrix) -> TensorHolder {
        match m {
            Matrix::F64(a) => TensorHolder::F64(Self::to_tensor(a)),
            Matrix::F32(a) => TensorHolder::F32(Self::to_tensor(a)),
            Matrix::I32(a) => TensorHolder::I32(Self::to_tensor(a)),
            Matrix::U8(a) => TensorHolder::U8(Self::to_tensor(a)),
            Matrix::I8(a) => TensorHolder::I8(Self::to_tensor(a)),
            Matrix::String(a) => TensorHolder::String(Self::to_tensor(a)),
        }
    }
}

fn tensor_to_matrix<T: ::tensorflow::TensorType>(tensor: &Tensor<T>) -> Result<ArrayD<T>> {
    let shape: Vec<usize> = tensor.dims().iter().map(|d| *d as _).collect();
    Ok(::ndarray::Array::from_iter(tensor.iter().cloned()).into_shape(shape)?)
}

impl Tensorflow {
    /// Executes the graph in one batch.
    pub fn run(&mut self, inputs: Vec<(&str, Matrix)>, output_name: &str) -> Result<Vec<Matrix>> {
        let tensors: Vec<(&str, TensorHolder)> = inputs
            .into_iter()
            .map(|(name, mat)| (name, mat.into()))
            .collect();

        let mut step = StepWithGraph::new();
        for t in &tensors {
            let op = self.graph.operation_by_name_required(t.0)?;
            match t.1 {
                TensorHolder::F64(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::F32(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::I32(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::U8(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::I8(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::String(ref it) => step.add_input(&op, 0, &it),
            }
        }

        let token = step.request_output(&self.graph.operation_by_name_required(output_name)?, 0);
        self.session.run(&mut step)?;

        let output_type = step.output_data_type(0).unwrap();
        convert_output(&mut step, output_type, token)
    }

    /// Executes the graph in one batch, and returns the output for every node but the inputs.
    pub fn run_get_all(&mut self, inputs: Vec<(&str, Matrix)>) -> Result<HashMap<String, Vec<Matrix>>> {
        let mut tensors: Vec<(&str, TensorHolder)> = Vec::new();
        let mut excluded = HashSet::new();

        for (name, mat) in inputs {
            tensors.push((name, mat.into()));
            excluded.insert(name.to_string());
        }

        let mut step = StepWithGraph::new();
        for t in &tensors {
            let op = self.graph.operation_by_name_required(t.0)?;
            match t.1 {
                TensorHolder::F64(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::F32(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::I32(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::U8(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::I8(ref it) => step.add_input(&op, 0, &it),
                TensorHolder::String(ref it) => step.add_input(&op, 0, &it),
            }
        }

        // Request the output of every node that's not an input.
        let mut tokens = HashMap::new();
        for operation in self.graph.operation_iter() {
            let name = operation.name()?;

            if excluded.contains(&name) {
                continue;
            }

            tokens.insert(name, step.request_output(&operation, 0));
        }

        // Execute the graph using tensorflow.
        self.session.run(&mut step)?;

        // Return the output for every node.
        let output_type = step.output_data_type(0).unwrap();
        let mut outputs = HashMap::new();
        for (name, token) in tokens {
            outputs.insert(name, convert_output(&mut step, output_type, token)?);
        }

        Ok(outputs)
    }
}

/// Converts the output of a Tensorflow node into a Vec<Matrix>.
fn convert_output(step: &mut StepWithGraph, output_type: DataType, output: OutputToken) -> Result<Vec<Matrix>> {
    macro_rules! convert {
        ($dt:ident) => (Matrix::$dt(tensor_to_matrix(&step.take_output(output)?)?))
    };

    let matrix = match output_type {
        DataType::Float => convert!(F32),
        DataType::UInt8 => convert!(U8),
        DataType::Int8 => convert!(I8),
        DataType::String => convert!(String),
        DataType::Int32 => convert!(I32),
        t => bail!("Missing tensor to matrix for type {:?}", t),
    };

    Ok(vec![matrix])
}