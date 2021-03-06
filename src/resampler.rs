use euclid::Size2D;
use num::rational::Ratio;
use parser::Component;

type ResampleFunc = fn(&[u8], Size2D<usize>, usize, usize, usize, &mut [u8]);

pub struct Resampler {
    resample_funcs: Vec<ResampleFunc>,
    sizes: Vec<Size2D<usize>>,
    row_strides: Vec<usize>,
}

impl Resampler {
    pub fn new(components: &[Component]) -> Option<Resampler> {
        let h_max = components.iter().map(|c| c.horizontal_sampling_factor).max().unwrap();
        let v_max = components.iter().map(|c| c.vertical_sampling_factor).max().unwrap();
        let resample_funcs: Vec<Option<ResampleFunc>> =
                components.iter()
                          .map(|component| choose_resampling_func(component, h_max, v_max))
                          .collect();

        if resample_funcs.iter().any(|func| func.is_none()) {
            None
        }
        else {
            Some(Resampler {
                resample_funcs: resample_funcs.iter().map(|func| func.unwrap()).collect(),
                sizes: components.iter().map(|comp| Size2D::new(comp.size.width as usize, comp.size.height as usize)).collect(),
                row_strides: components.iter().map(|comp| comp.block_size.width as usize * 8).collect(),
            })
        }
    }

    pub fn resample_and_interleave_row(&self, component_data: &[Vec<u8>], row: usize, output_width: usize, output: &mut [u8]) {
        let component_count = component_data.len();
        let mut line_buffer = vec![0u8; output_width + 1];

        for i in 0 .. component_count {
            self.resample_funcs[i](&component_data[i],
                                   self.sizes[i],
                                   self.row_strides[i],
                                   row,
                                   output_width,
                                   &mut line_buffer);

            for x in 0 .. output_width {
                output[x * component_count + i] = line_buffer[x];
            }
        }
    }
}

fn choose_resampling_func(component: &Component, h_max: u8, v_max: u8) -> Option<ResampleFunc> {
    let horizontal_scale_factor = Ratio::new(h_max, component.horizontal_sampling_factor);
    let vertical_scale_factor = Ratio::new(v_max, component.vertical_sampling_factor);

    if !horizontal_scale_factor.is_integer() || !vertical_scale_factor.is_integer() {
        return None;
    }

    match (horizontal_scale_factor.to_integer(), vertical_scale_factor.to_integer()) {
        (1, 1) => Some(resample_row_1),
        (2, 1) => Some(resample_row_h_2_bilinear),
        (1, 2) => Some(resample_row_v_2_bilinear),
        (2, 2) => Some(resample_row_hv_2_bilinear),
        _ => None,
    }
}

fn resample_row_1(input: &[u8], _input_size: Size2D<usize>, row_stride: usize, row: usize, output_width: usize, output: &mut [u8]) {
    let input = &input[row * row_stride ..];

    for i in 0 .. output_width {
        output[i] = input[i];
    }
}

fn resample_row_h_2_bilinear(input: &[u8], input_size: Size2D<usize>, row_stride: usize, row: usize, _output_width: usize, output: &mut [u8]) {
    let input = &input[row * row_stride ..];

    if input_size.width == 1 {
        output[0] = input[0];
        output[1] = input[0];
        return;
    }

    output[0] = input[0];
    output[1] = ((input[0] as u32 * 3 + input[1] as u32 + 2) >> 2) as u8;

    for i in 1 .. input_size.width - 1 {
        let sample = 3 * input[i] as u32 + 2;
        output[i * 2]     = ((sample + input[i - 1] as u32) >> 2) as u8;
        output[i * 2 + 1] = ((sample + input[i + 1] as u32) >> 2) as u8;
    }

    output[(input_size.width - 1) * 2] = ((input[input_size.width - 1] as u32 * 3 + input[input_size.width - 2] as u32 + 2) >> 2) as u8;
    output[(input_size.width - 1) * 2 + 1] = input[input_size.width - 1];
}

fn resample_row_v_2_bilinear(input: &[u8], input_size: Size2D<usize>, row_stride: usize, row: usize, output_width: usize, output: &mut [u8]) {
    let row_near = row as f32 / 2.0;
    // If row_near's fractional is 0.0 we want row_far to be the previous row and if it's 0.5 we
    // want it to be the next row.
    let row_far = (row_near + row_near.fract() * 3.0 - 0.25).min((input_size.height - 1) as f32);

    let input_near = &input[row_near as usize * row_stride ..];
    let input_far = &input[row_far as usize * row_stride ..];

    for i in 0 .. output_width {
        output[i] = ((3 * input_near[i] as u32 + input_far[i] as u32 + 2) >> 2) as u8;
    }
}

fn resample_row_hv_2_bilinear(input: &[u8], input_size: Size2D<usize>, row_stride: usize, row: usize, _output_width: usize, output: &mut [u8]) {
    let row_near = row as f32 / 2.0;
    // If row_near's fractional is 0.0 we want row_far to be the previous row and if it's 0.5 we
    // want it to be the next row.
    let row_far = (row_near + row_near.fract() * 3.0 - 0.25).min((input_size.height - 1) as f32);

    let input_near = &input[row_near as usize * row_stride ..];
    let input_far = &input[row_far as usize * row_stride ..];

    if input_size.width == 1 {
        let value = ((3 * input_near[0] as u32 + input_far[0] as u32 + 2) >> 2) as u8;
        output[0] = value;
        output[1] = value;
        return;
    }

    let mut t1 = 3 * input_near[0] as u32 + input_far[0] as u32;
    output[0] = ((t1 + 2) >> 2) as u8;

    for i in 1 .. input_size.width {
        let t0 = t1;
        t1 = 3 * input_near[i] as u32 + input_far[i] as u32;

        output[i * 2 - 1] = ((3 * t0 + t1 + 8) >> 4) as u8;
        output[i * 2]     = ((3 * t1 + t0 + 8) >> 4) as u8;
    }

    output[input_size.width * 2 - 1] = ((t1 + 2) >> 2) as u8;
}
