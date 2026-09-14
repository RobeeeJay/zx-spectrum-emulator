//! The network: pictures in, a preference for each action and a guess at how
//! well things are going out.
//!
//! The shape that learnt Atari games from their pixels: three convolutions
//! over the stacked frames, a layer of 256, and two heads — one scoring each
//! action, one estimating the reward to come, which is what PPO trains the
//! choices against. Its size follows from what the network is shown, so a
//! smaller picture is a smaller, quicker network.

use burn::nn::conv::{Conv2d, Conv2dConfig};
use burn::nn::{Linear, LinearConfig};
use burn::prelude::*;
use burn::tensor::activation::relu;

use super::sight::Sight;

#[derive(Module, Debug)]
pub struct Net<B: Backend> {
    conv1: Conv2d<B>,
    conv2: Conv2d<B>,
    conv3: Conv2d<B>,
    hidden: Linear<B>,
    policy: Linear<B>,
    value: Linear<B>,
}

/// What a convolution leaves of `size`, with no padding.
fn after(size: usize, kernel: usize, stride: usize) -> usize {
    (size - kernel) / stride + 1
}

/// The smallest picture the network can be shown: its first two layers take
/// 8 then 4 pixels, and the third 3, and less than this leaves nothing.
pub const SMALLEST: usize = 36;

impl<B: Backend> Net<B> {
    pub fn new(sight: &Sight, actions: usize, device: &B::Device) -> Result<Net<B>, String> {
        let channels = sight.channels() * sight.frames.max(1);
        let (h, w) = (sight.height(), sight.width());
        if h < SMALLEST || w < SMALLEST {
            return Err(format!(
                "a {w}x{h} picture is too small for the network, which needs {SMALLEST} \
                 pixels each way: shrink it less"
            ));
        }
        let (h1, w1) = (after(h, 8, 4), after(w, 8, 4));
        let (h2, w2) = (after(h1, 4, 2), after(w1, 4, 2));
        let (h3, w3) = (after(h2, 3, 1), after(w2, 3, 1));
        Ok(Net {
            conv1: Conv2dConfig::new([channels, 32], [8, 8])
                .with_stride([4, 4])
                .init(device),
            conv2: Conv2dConfig::new([32, 64], [4, 4])
                .with_stride([2, 2])
                .init(device),
            conv3: Conv2dConfig::new([64, 64], [3, 3]).init(device),
            hidden: LinearConfig::new(64 * h3 * w3, 256).init(device),
            policy: LinearConfig::new(256, actions.max(1)).init(device),
            value: LinearConfig::new(256, 1).init(device),
        })
    }

    /// A batch of pictures, `[games, channels, height, width]` scaled to 0..1,
    /// to the action scores `[games, actions]` and values `[games, 1]`.
    pub fn forward(&self, pictures: Tensor<B, 4>) -> (Tensor<B, 2>, Tensor<B, 2>) {
        let x = relu(self.conv1.forward(pictures));
        let x = relu(self.conv2.forward(x));
        let x = relu(self.conv3.forward(x));
        let x: Tensor<B, 2> = x.flatten(1, 3);
        let x = relu(self.hidden.forward(x));
        (self.policy.forward(x.clone()), self.value.forward(x))
    }
}

/// Observations, bytes as `sight::look` made them, as a batch the network takes.
pub fn batch<B: Backend>(
    observations: &[&[u8]],
    sight: &Sight,
    device: &B::Device,
) -> Tensor<B, 4> {
    let channels = sight.channels() * sight.frames.max(1);
    let data: Vec<f32> = observations
        .iter()
        .flat_map(|o| o.iter().map(|b| *b as f32 / 255.0))
        .collect();
    Tensor::from_data(
        TensorData::new(
            data,
            [observations.len(), channels, sight.height(), sight.width()],
        ),
        device,
    )
}
