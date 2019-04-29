#![cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))] // TODO later

use nuklear::{Buffer as NkBuffer, Context, ConvertConfig, DrawVertexLayoutAttribute, DrawVertexLayoutElements, DrawVertexLayoutFormat, Handle, Size, Vec2};
use std::{
    mem::{size_of, size_of_val},
    slice::from_raw_parts,
    str::from_utf8,
};
use wgpu::*;

pub const TEXTURE_FORMAT: TextureFormat = TextureFormat::Bgra8Unorm;

#[allow(dead_code)]
struct Vertex {
    pos: [f32; 2], // "Position",
    tex: [f32; 2], // "TexCoord",
    col: [u8; 4],  // "Color",
}
#[allow(dead_code)]
struct WgpuTexture {
    texture: Texture,
    sampler: Sampler,

    pub bind_group: BindGroup,
}

type Ortho = [[f32; 4]; 4];

impl WgpuTexture {
    pub fn new(device: &mut Device, drawer: &Drawer, image: &[u8], width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            size: Extent3d { width: width, height: height, depth: 1 },
            array_size: 1,
            dimension: TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: TextureUsageFlags::SAMPLED | TextureUsageFlags::TRANSFER_DST,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            r_address_mode: AddressMode::ClampToEdge,
            s_address_mode: AddressMode::ClampToEdge,
            t_address_mode: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            lod_min_clamp: -100.0,
            lod_max_clamp: 100.0,
            max_anisotropy: 0,
            compare_function: CompareFunction::Always,
            border_color: BorderColor::TransparentBlack,
        });

        let bytes = image.len();
        let buffer = device.create_buffer_mapped(bytes, BufferUsageFlags::TRANSFER_SRC);
        let buffer = buffer.fill_from_slice(image);

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { todo: 0 });

        let pixel_size = bytes as u32 / width / height;
        encoder.copy_buffer_to_texture(
            BufferCopyView {
                buffer: &buffer,
                offset: 0,
                row_pitch: pixel_size * width,
                image_height: height,
            },
            TextureCopyView {
                texture: &texture,
                level: 0,
                slice: 0,
                origin: Origin3d { x: 0.0, y: 0.0, z: 0.0 },
            },
            Extent3d { width, height, depth: 1 },
        );

        device.get_queue().submit(&[encoder.finish()]);

        WgpuTexture {
            bind_group: device.create_bind_group(&BindGroupDescriptor {
                layout: &drawer.tla,
                bindings: &[
                    wgpu::Binding {
                        binding: 0,
                        resource: BindingResource::TextureView(&texture.create_default_view()),
                    },
                    wgpu::Binding {
                        binding: 1,
                        resource: BindingResource::Sampler(&sampler),
                    },
                ],
            }),
            sampler,
            texture,
        }
    }
}

pub struct Drawer {
    cmd: NkBuffer,
    pso: RenderPipeline,
    tla: BindGroupLayout,
    tex: Vec<WgpuTexture>,
    ubf: Buffer,
    ubg: BindGroup,
    vsz: usize,
    esz: usize,
    vle: DrawVertexLayoutElements,

    pub col: Option<Color>,
}

impl Drawer {
    pub fn new(device: &mut Device, col: Color, texture_count: usize, vbo_size: usize, ebo_size: usize, command_buffer: NkBuffer) -> Drawer {
        let vs: &[u8] = include_bytes!("../shaders/vs.fx");
        let fs: &[u8] = include_bytes!("../shaders/ps.fx");

        let vs = device.create_shader_module(compile_glsl(from_utf8(vs).unwrap(), glsl_to_spirv::ShaderType::Vertex).as_slice());
        let fs = device.create_shader_module(compile_glsl(from_utf8(fs).unwrap(), glsl_to_spirv::ShaderType::Fragment).as_slice());

        let ubf = device.create_buffer(&BufferDescriptor {
            size: size_of::<Ortho>() as u32,
            usage: BufferUsageFlags::UNIFORM | BufferUsageFlags::TRANSFER_DST,
        });
        let ubg = BindGroupLayoutDescriptor {
            bindings: &[BindGroupLayoutBinding {
                binding: 0,
                visibility: ShaderStageFlags::VERTEX,
                ty: BindingType::UniformBuffer,
            }],
        };

        let tbg = BindGroupLayoutDescriptor {
            bindings: &[
                BindGroupLayoutBinding {
                    binding: 0,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::SampledTexture,
                },
                BindGroupLayoutBinding {
                    binding: 1,
                    visibility: ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Sampler,
                },
            ],
        };
        let tla = device.create_bind_group_layout(&tbg);
        let ula = device.create_bind_group_layout(&ubg);

        Drawer {
            cmd: command_buffer,
            col: Some(col),
            pso: device.create_render_pipeline(&RenderPipelineDescriptor {
                layout: &device.create_pipeline_layout(&PipelineLayoutDescriptor { bind_group_layouts: &[&ula, &tla] }),
                vertex_stage: PipelineStageDescriptor { module: &vs, entry_point: "main" },
                fragment_stage: PipelineStageDescriptor { module: &fs, entry_point: "main" },
                rasterization_state: RasterizationStateDescriptor {
                    front_face: FrontFace::Cw,
                    cull_mode: CullMode::None,
                    depth_bias: 0,
                    depth_bias_slope_scale: 0.0,
                    depth_bias_clamp: 0.0,
                },
                primitive_topology: PrimitiveTopology::TriangleList,
                color_states: &[ColorStateDescriptor {
                    format: TEXTURE_FORMAT,
                    color: BlendDescriptor {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendDescriptor {
                        src_factor: BlendFactor::OneMinusDstAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    write_mask: ColorWriteFlags::ALL,
                }],
                depth_stencil_state: None,
                index_format: IndexFormat::Uint16,
                vertex_buffers: &[VertexBufferDescriptor {
                    stride: (size_of::<Vertex>()) as u32,
                    step_mode: InputStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttributeDescriptor {
                            format: VertexFormat::Float2,
                            attribute_index: 0,
                            offset: 0,
                        },
                        wgpu::VertexAttributeDescriptor {
                            format: VertexFormat::Float2,
                            attribute_index: 1,
                            offset: 8,
                        },
                        wgpu::VertexAttributeDescriptor {
                            format: VertexFormat::Uint,
                            attribute_index: 2,
                            offset: 16,
                        },
                    ],
                }],
                sample_count: 1,
            }),
            tex: Vec::with_capacity(texture_count + 1),
            vsz: vbo_size,
            esz: ebo_size,
            ubg: device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &ula,
                bindings: &[Binding {
                    binding: 0,
                    resource: BindingResource::Buffer {
                        buffer: &ubf,
                        range: 0..(size_of::<Ortho>() as u32),
                    },
                }],
            }),
            ubf: ubf,
            vle: DrawVertexLayoutElements::new(&[
                (DrawVertexLayoutAttribute::Position, DrawVertexLayoutFormat::Float, 0),
                (DrawVertexLayoutAttribute::TexCoord, DrawVertexLayoutFormat::Float, size_of::<f32>() as Size * 2),
                (DrawVertexLayoutAttribute::Color, DrawVertexLayoutFormat::B8G8R8A8, size_of::<f32>() as Size * 4),
                (DrawVertexLayoutAttribute::AttributeCount, DrawVertexLayoutFormat::Count, 0),
            ]),
            tla: tla,
        }
    }

    pub fn add_texture(&mut self, device: &mut Device, image: &[u8], width: u32, height: u32) -> Handle {
        self.tex.push(WgpuTexture::new(device, self, image, width, height));
        Handle::from_id(self.tex.len() as i32)
    }

    pub fn draw(&mut self, ctx: &mut Context, cfg: &mut ConvertConfig, encoder: &mut CommandEncoder, view: &TextureView, device: &mut Device, width: u32, height: u32, scale: Vec2) {
        let ortho: Ortho = [
            [2.0f32 / width as f32, 0.0f32, 0.0f32, 0.0f32],
            [0.0f32, -2.0f32 / height as f32, 0.0f32, 0.0f32],
            [0.0f32, 0.0f32, -1.0f32, 0.0f32],
            [-1.0f32, 1.0f32, 0.0f32, 1.0f32],
        ];
        let ubf_size = size_of_val(&ortho);
        cfg.set_vertex_layout(&self.vle);
        cfg.set_vertex_size(size_of::<Vertex>());

        let mut vbf = device.create_buffer_mapped(self.vsz, BufferUsageFlags::VERTEX | BufferUsageFlags::TRANSFER_DST);
        let mut ebf = device.create_buffer_mapped(self.esz, BufferUsageFlags::INDEX | BufferUsageFlags::TRANSFER_DST);
        let ubf = device.create_buffer_mapped(ubf_size, BufferUsageFlags::UNIFORM | BufferUsageFlags::TRANSFER_DST);
        {
            let mut vbuf = NkBuffer::with_fixed(&mut vbf.data);
            let mut ebuf = NkBuffer::with_fixed(&mut ebf.data);

            ctx.convert(&mut self.cmd, &mut vbuf, &mut ebuf, cfg);
            
            let vbf = unsafe { std::slice::from_raw_parts_mut(vbf.data as *mut _ as *mut Vertex, vbf.data.len()/std::mem::size_of::<Vertex>()) };
            
            for v in vbf.iter_mut() {
                v.pos[1] = height as f32 - v.pos[1];
            }
        }
        let vbf = vbf.finish();
        let ebf = ebf.finish();
        let ubf = ubf.fill_from_slice(as_typed_slice(&ortho));

        encoder.copy_buffer_to_buffer(&ubf, 0, &self.ubf, 0, ubf_size as u32);

        let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
            color_attachments: &[RenderPassColorAttachmentDescriptor {
                attachment: &view,
                load_op: match self.col {
                    Some(_) => wgpu::LoadOp::Clear,
                    _ => wgpu::LoadOp::Load,
                },
                store_op: StoreOp::Store,
                clear_color: self.col.unwrap_or(Color { r: 1.0, g: 2.0, b: 3.0, a: 1.0 }),
            }],
            depth_stencil_attachment: None,
        });
        rpass.set_pipeline(&self.pso);

        rpass.set_vertex_buffers(&[(&vbf, 0)]);
        rpass.set_index_buffer(&ebf, 0);

        rpass.set_bind_group(0, &self.ubg);

        let mut start = 0;
        let mut end;

        for cmd in ctx.draw_command_iterator(&self.cmd) {
            if cmd.elem_count() < 1 {
                continue;
            }

            let id = cmd.texture().id().unwrap();
            let res = self.find_res(id).unwrap();

            end = start + cmd.elem_count();

            rpass.set_bind_group(1, &res.bind_group);

            rpass.set_scissor_rect((cmd.clip_rect().x * scale.x) as u32, (cmd.clip_rect().y * scale.y) as u32, (cmd.clip_rect().w * scale.x) as u32, (cmd.clip_rect().h * scale.y) as u32);

            rpass.draw_indexed(start..end, 0 as i32, 0..1);

            start = end;
        }
    }

    fn find_res(&self, id: i32) -> Option<&WgpuTexture> {
        if id > 0 && id as usize <= self.tex.len() {
            self.tex.get((id - 1) as usize)
        } else {
            None
        }
    }
}

fn as_typed_slice<T>(data: &[T]) -> &[u8] {
    unsafe { from_raw_parts(data.as_ptr() as *const u8, data.len() * size_of::<T>()) }
}
fn compile_glsl(code: &str, ty: glsl_to_spirv::ShaderType) -> Vec<u8> {
    use std::io::Read;

    let mut output = glsl_to_spirv::compile(code, ty).unwrap();
    let mut spv = Vec::new();
    output.read_to_end(&mut spv).unwrap();
    spv
}
