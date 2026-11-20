use super::*;

// ============================================================================
// 3. Pixel Bender Shaders (`flash.display.Shader*`, `flash.filters.ShaderFilter`)
// ============================================================================

pub fn validate_shader_precision(value: &str) -> Result<&str, Fte3dError> {
    if matches!(value, "full" | "fast") {
        Ok(value)
    } else {
        Err(Fte3dError::Argument(format!(
            "Invalid ShaderPrecision: '{value}'."
        )))
    }
}

pub fn shader_param_component_count(param_type: &str) -> Result<usize, Fte3dError> {
    match param_type {
        "float" | "int" | "bool" => Ok(1),
        "float2" | "int2" | "bool2" => Ok(2),
        "float3" | "int3" | "bool3" => Ok(3),
        "float4" | "int4" | "bool4" | "matrix2x2" => Ok(4),
        "matrix3x3" => Ok(9),
        "matrix4x4" => Ok(16),
        _ => Err(Fte3dError::Argument(format!(
            "Invalid ShaderParameterType: '{param_type}'."
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderInputMeta {
    pub name: String,
    pub index: u32,
    pub channels: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderParamMeta {
    pub name: String,
    pub index: u32,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub default_value: Vec<f64>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub reg: u16,
    #[serde(default = "default_pb_mask")]
    pub mask: u8,
}

fn default_pb_mask() -> u8 {
    0x0F
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderMetadata {
    pub name: String,
    pub version: i32,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub inputs: Vec<ShaderInputMeta>,
    pub parameters: Vec<ShaderParamMeta>,
    #[serde(default)]
    pub op: String,
}

#[derive(Debug, Clone)]
enum PbjInst {
    LoadConst {
        dst_reg: u16,
        dst_mask: u8,
        value: f32,
    },
    Sample {
        dst_reg: u16,
        dst_mask: u8,
        coord_reg: u16,
        coord_swz: [u8; 2],
        tex_index: u8,
        linear: bool,
    },
    Alu {
        op: u8,
        dst_reg: u16,
        dst_mask: u8,
        dst_len: u8,
        src_reg: u16,
        src_swz: [u8; 4],
    },
    If {
        src_reg: u16,
        src_swz: [u8; 4],
    },
    Else,
    EndIf,
}

#[derive(Debug, Clone)]
struct ParsedPbj {
    meta: ShaderMetadata,
    out_coord_reg: u16,
    out_coord_mask: u8,
    out_pixel_reg: u16,
    out_pixel_mask: u8,
    out_channels: u32,
    code: Vec<PbjInst>,
}

fn pb_type_name_and_len(code: u8) -> Option<(&'static str, usize)> {
    match code {
        0x01 => Some(("float", 1)),
        0x02 => Some(("float2", 2)),
        0x03 => Some(("float3", 3)),
        0x04 => Some(("float4", 4)),
        0x05 => Some(("matrix2x2", 4)),
        0x06 => Some(("matrix3x3", 9)),
        0x07 => Some(("matrix4x4", 16)),
        0x08 => Some(("int", 1)),
        0x09 => Some(("int2", 2)),
        0x0A => Some(("int3", 3)),
        0x0B => Some(("int4", 4)),
        0x0C => Some(("bool", 1)),
        0x0D => Some(("bool2", 2)),
        0x0E => Some(("bool3", 3)),
        0x0F => Some(("bool4", 4)),
        _ => None,
    }
}

fn read_cstring(bytes: &[u8], pos: &mut usize) -> Result<String, Fte3dError> {
    let start = *pos;
    while *pos < bytes.len() && bytes[*pos] != 0 {
        *pos += 1;
    }
    if *pos >= bytes.len() {
        return Err(Fte3dError::Argument(
            "Invalid Shader byteCode: unterminated string in Pixel Bender stream.".into(),
        ));
    }
    let s = String::from_utf8_lossy(&bytes[start..*pos]).into_owned();
    *pos += 1;
    Ok(s)
}

fn read_pb_typed_value(
    bytes: &[u8],
    pos: &mut usize,
    pb_type: u8,
) -> Result<serde_json::Value, Fte3dError> {
    if pb_type == 0x34 {
        let s = read_cstring(bytes, pos)?;
        return Ok(serde_json::Value::String(s));
    }
    let Some((type_name, count)) = pb_type_name_and_len(pb_type) else {
        return Err(Fte3dError::Argument(format!(
            "Invalid Shader byteCode: unsupported metadata value type 0x{pb_type:02X}."
        )));
    };
    let mut vals = Vec::with_capacity(count);
    for _ in 0..count {
        if type_name.starts_with("int") || type_name.starts_with("bool") {
            if *pos + 2 > bytes.len() {
                return Err(Fte3dError::Argument(
                    "Invalid Shader byteCode: truncated int/bool metadata.".into(),
                ));
            }
            let v = i16::from_be_bytes([bytes[*pos], bytes[*pos + 1]]);
            *pos += 2;
            if type_name.starts_with("bool") {
                vals.push(serde_json::Value::Bool(v != 0));
            } else {
                vals.push(serde_json::json!(v));
            }
        } else {
            if *pos + 4 > bytes.len() {
                return Err(Fte3dError::Argument(
                    "Invalid Shader byteCode: truncated float metadata.".into(),
                ));
            }
            let bits = u32::from_be_bytes([
                bytes[*pos],
                bytes[*pos + 1],
                bytes[*pos + 2],
                bytes[*pos + 3],
            ]);
            *pos += 4;
            let f = f32::from_bits(bits) as f64;
            vals.push(serde_json::json!(f));
        }
    }
    if count == 1 {
        Ok(vals.into_iter().next().unwrap_or(serde_json::Value::Null))
    } else {
        Ok(serde_json::Value::Array(vals))
    }
}

fn parse_pbj_or_json(bytes: &[u8]) -> Result<ParsedPbj, Fte3dError> {
    if bytes.is_empty() {
        return Err(Fte3dError::Argument(
            "Shader.byteCode must not be empty.".into(),
        ));
    }
    // 1. Portable JSON shader descriptor (starts with '{' after optional whitespace)
    let trimmed = {
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        &bytes[i..]
    };
    if trimmed.first() == Some(&b'{') {
        let raw: serde_json::Value = serde_json::from_slice(trimmed).map_err(|e| {
            Fte3dError::Argument(format!("Invalid JSON Shader byteCode descriptor: {e}"))
        })?;
        let name = raw
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("NeoFlashShader")
            .to_string();
        let version = raw.get("version").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
        let op = raw
            .get("op")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();
        let mut metadata = BTreeMap::new();
        if let Some(obj) = raw.get("metadata").and_then(|v| v.as_object()) {
            for (k, v) in obj {
                metadata.insert(k.clone(), v.clone());
            }
        }
        let mut inputs = Vec::new();
        if let Some(arr) = raw.get("inputs").and_then(|v| v.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let in_name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("src")
                    .to_string();
                let index = item
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(idx as u64) as u32;
                let channels = item
                    .get("channels")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(4)
                    .clamp(1, 4) as u32;
                inputs.push(ShaderInputMeta {
                    name: in_name,
                    index,
                    channels,
                });
            }
        }
        let mut parameters = Vec::new();
        if let Some(arr) = raw.get("parameters").and_then(|v| v.as_array()) {
            for (idx, item) in arr.iter().enumerate() {
                let p_name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("param")
                    .to_string();
                let p_type = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("float")
                    .to_string();
                let comp_len = shader_param_component_count(&p_type)?;
                let index = item
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(idx as u64) as u32;
                let mut default_value = Vec::new();
                if let Some(dv) = item.get("defaultValue").and_then(|v| v.as_array()) {
                    for x in dv {
                        if let Some(f) = x.as_f64() {
                            default_value.push(f);
                        } else if let Some(b) = x.as_bool() {
                            default_value.push(if b { 1.0 } else { 0.0 });
                        }
                    }
                }
                while default_value.len() < comp_len {
                    default_value.push(0.0);
                }
                default_value.truncate(comp_len);
                let mut p_meta = BTreeMap::new();
                if let Some(m) = item.get("metadata").and_then(|v| v.as_object()) {
                    for (k, v) in m {
                        p_meta.insert(k.clone(), v.clone());
                    }
                }
                parameters.push(ShaderParamMeta {
                    name: p_name,
                    index,
                    param_type: p_type,
                    default_value,
                    metadata: p_meta,
                    reg: (idx + 2) as u16,
                    mask: 0x0F,
                });
            }
        }
        let out_channels = raw
            .get("outChannels")
            .and_then(|v| v.as_u64())
            .unwrap_or(4)
            .clamp(1, 4) as u32;
        return Ok(ParsedPbj {
            meta: ShaderMetadata {
                name,
                version,
                metadata,
                inputs,
                parameters,
                op,
            },
            out_coord_reg: 0,
            out_coord_mask: 0x0C,
            out_pixel_reg: 1,
            out_pixel_mask: 0x0F,
            out_channels,
            code: Vec::new(),
        });
    }

    // 2. Binary Pixel Bender (.pbj) stream: must begin with 0xA5 version opcode.
    if bytes[0] != 0xA5 || bytes.len() < 5 {
        return Err(Fte3dError::Argument(
            "Invalid Shader byteCode: expected Pixel Bender (.pbj) header (0xA5) or JSON shader descriptor.".into(),
        ));
    }

    let mut pos = 0usize;
    let mut version = 1i32;
    let mut name = String::from("PixelBenderKernel");
    let mut metadata = BTreeMap::new();
    let mut inputs = Vec::new();
    let mut parameters: Vec<ShaderParamMeta> = Vec::new();
    let mut out_coord_reg = 0u16;
    let mut out_coord_mask = 0x0Cu8;
    let mut out_pixel_reg = 1u16;
    let mut out_pixel_mask = 0x0Fu8;
    let mut out_channels = 4u32;
    let mut code = Vec::new();
    let mut last_param_type: Option<u8> = None;

    while pos < bytes.len() {
        let op = bytes[pos];
        pos += 1;
        match op {
            0xA5 => {
                if pos + 4 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA5 version record.".into(),
                    ));
                }
                version = i32::from_le_bytes([
                    bytes[pos],
                    bytes[pos + 1],
                    bytes[pos + 2],
                    bytes[pos + 3],
                ]);
                pos += 4;
            }
            0xA4 => {
                if pos + 2 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA4 kernel name.".into(),
                    ));
                }
                let len = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as usize;
                pos += 2;
                if pos + len > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: kernel name exceeds stream length.".into(),
                    ));
                }
                name = String::from_utf8_lossy(&bytes[pos..pos + len]).into_owned();
                pos += len;
            }
            0xA0 => {
                if pos >= bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA0 metadata.".into(),
                    ));
                }
                let mtype = bytes[pos];
                pos += 1;
                let key = read_cstring(bytes, &mut pos)?;
                let val = read_pb_typed_value(bytes, &mut pos, mtype)?;
                metadata.insert(key, val);
            }
            0xA3 => {
                if pos + 2 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA3 texture input.".into(),
                    ));
                }
                let index = bytes[pos] as u32;
                let channels = (bytes[pos + 1] as u32).clamp(1, 4);
                pos += 2;
                let in_name = read_cstring(bytes, &mut pos)?;
                inputs.push(ShaderInputMeta {
                    name: in_name,
                    index,
                    channels,
                });
            }
            0xA1 => {
                if pos + 5 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA1 parameter.".into(),
                    ));
                }
                let qualifier = bytes[pos];
                let pb_type = bytes[pos + 1];
                let reg = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]);
                let mask = bytes[pos + 4];
                pos += 5;
                let p_name = read_cstring(bytes, &mut pos)?;
                let Some((type_name, comp_len)) = pb_type_name_and_len(pb_type) else {
                    return Err(Fte3dError::Argument(format!(
                        "Invalid Shader byteCode: unknown parameter type 0x{pb_type:02X}."
                    )));
                };
                if qualifier == 2 {
                    // Output pixel parameter (typically "dst")
                    out_pixel_reg = reg;
                    out_pixel_mask = if mask == 0 { 0x0F } else { mask };
                    out_channels = comp_len.clamp(1, 4) as u32;
                    last_param_type = None;
                } else if p_name == "_OutCoord" {
                    out_coord_reg = reg;
                    out_coord_mask = if mask == 0 { 0x0C } else { mask };
                    last_param_type = None;
                } else {
                    let index = parameters.len() as u32;
                    parameters.push(ShaderParamMeta {
                        name: p_name,
                        index,
                        param_type: type_name.to_string(),
                        default_value: vec![0.0; comp_len],
                        metadata: BTreeMap::new(),
                        reg,
                        mask: if mask == 0 { 0x0F } else { mask },
                    });
                    last_param_type = Some(pb_type);
                }
            }
            0xA2 => {
                if pos >= bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated 0xA2 parameter metadata.".into(),
                    ));
                }
                let mtype = bytes[pos];
                pos += 1;
                let key = read_cstring(bytes, &mut pos)?;
                let effective_type = if mtype == 0x34 {
                    0x34
                } else {
                    last_param_type.unwrap_or(mtype)
                };
                let val = read_pb_typed_value(bytes, &mut pos, effective_type)?;
                if let Some(last) = parameters.last_mut() {
                    if key == "defaultValue" {
                        if let Some(arr) = val.as_array() {
                            last.default_value = arr
                                .iter()
                                .map(|v| {
                                    v.as_f64()
                                        .or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 }))
                                        .unwrap_or(0.0)
                                })
                                .collect();
                        } else if let Some(f) = val.as_f64() {
                            last.default_value = vec![f];
                        } else if let Some(b) = val.as_bool() {
                            last.default_value = vec![if b { 1.0 } else { 0.0 }];
                        }
                    }
                    last.metadata.insert(key, val);
                }
            }
            0x32 | 0x33 => {
                // 0x32 loadFloat / 0x33 loadInt immediate (7 bytes after opcode)
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated immediate instruction.".into(),
                    ));
                }
                let dst_reg = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let dst_mask = bytes[pos + 2] >> 4;
                let value = if op == 0x33 {
                    i32::from_be_bytes([
                        bytes[pos + 3],
                        bytes[pos + 4],
                        bytes[pos + 5],
                        bytes[pos + 6],
                    ]) as f32
                } else {
                    f32::from_bits(u32::from_be_bytes([
                        bytes[pos + 3],
                        bytes[pos + 4],
                        bytes[pos + 5],
                        bytes[pos + 6],
                    ]))
                };
                pos += 7;
                code.push(PbjInst::LoadConst {
                    dst_reg,
                    dst_mask,
                    value,
                });
            }
            0x30 | 0x31 => {
                // sampleNearest / sampleLinear (7 bytes after opcode)
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated texture sample instruction.".into(),
                    ));
                }
                let dst_reg = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let dst_mask = bytes[pos + 2] >> 4;
                let coord_reg = u16::from_le_bytes([bytes[pos + 3], bytes[pos + 4]]);
                let swz_byte = bytes[pos + 5];
                let coord_swz = [(swz_byte >> 6) & 3, (swz_byte >> 4) & 3];
                let tex_index = bytes[pos + 6];
                pos += 7;
                code.push(PbjInst::Sample {
                    dst_reg,
                    dst_mask,
                    coord_reg,
                    coord_swz,
                    tex_index,
                    linear: op == 0x31,
                });
            }
            0x00 => {
                if pos + 7 > bytes.len() {
                    break;
                }
                pos += 7;
            }
            0x01..=0x2F | 0x37..=0x3D => {
                // Standard 8-byte PBJ ALU / vector op
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(format!(
                        "Invalid Shader byteCode: truncated ALU instruction 0x{op:02X}."
                    )));
                }
                let dst_reg = u16::from_le_bytes([bytes[pos], bytes[pos + 1]]);
                let mask_len = bytes[pos + 2];
                let dst_mask = mask_len >> 4;
                let dst_len = ((mask_len >> 2) & 0x03) + 1;
                let src_reg = u16::from_le_bytes([bytes[pos + 3], bytes[pos + 4]]);
                let swz = bytes[pos + 5];
                let src_swz = [(swz >> 6) & 3, (swz >> 4) & 3, (swz >> 2) & 3, swz & 3];
                pos += 7;
                code.push(PbjInst::Alu {
                    op,
                    dst_reg,
                    dst_mask,
                    dst_len,
                    src_reg,
                    src_swz,
                });
            }
            0x34..=0x36 => {
                // Control-flow opcodes (0x34 if, 0x35 else, 0x36 endif) — 7 bytes after opcode
                if pos + 7 > bytes.len() {
                    return Err(Fte3dError::Argument(
                        "Invalid Shader byteCode: truncated control-flow instruction.".into(),
                    ));
                }
                if op == 0x34 {
                    let src_reg = u16::from_le_bytes([bytes[pos + 3], bytes[pos + 4]]);
                    let swz = bytes[pos + 5];
                    let src_swz = [(swz >> 6) & 3, (swz >> 4) & 3, (swz >> 2) & 3, swz & 3];
                    code.push(PbjInst::If { src_reg, src_swz });
                } else if op == 0x35 {
                    code.push(PbjInst::Else);
                } else {
                    code.push(PbjInst::EndIf);
                }
                pos += 7;
            }
            other => {
                return Err(Fte3dError::Argument(format!(
                    "Invalid Shader byteCode: unknown Pixel Bender opcode 0x{other:02X}."
                )));
            }
        }
    }

    Ok(ParsedPbj {
        meta: ShaderMetadata {
            name,
            version,
            metadata,
            inputs,
            parameters,
            op: "pbj".into(),
        },
        out_coord_reg,
        out_coord_mask,
        out_pixel_reg,
        out_pixel_mask,
        out_channels,
        code,
    })
}

pub fn parse_shader_bytecode(bytes: &[u8]) -> Result<ShaderMetadata, Fte3dError> {
    Ok(parse_pbj_or_json(bytes)?.meta)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderRuntimeInput {
    pub name: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "default_four")]
    pub channels: u32,
    #[serde(default)]
    pub floats: Vec<f32>,
    #[serde(default)]
    pub rgba_base64: Option<String>,
}

fn default_four() -> u32 {
    4
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderEvalRequest {
    pub bytecode_base64: String,
    #[serde(default = "default_full_precision")]
    pub precision_hint: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub inputs: Vec<ShaderRuntimeInput>,
    #[serde(default)]
    pub parameters: BTreeMap<String, Vec<f64>>,
}

fn default_full_precision() -> String {
    "full".into()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderEvalResult {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub floats: Vec<f32>,
    pub rgba_base64: String,
}

#[derive(Debug, Clone)]
struct DecodedShaderInput {
    width: u32,
    height: u32,
    channels: u32,
    floats: Vec<f32>,
}

impl DecodedShaderInput {
    fn sample_nearest(&self, x: f32, y: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 || self.floats.is_empty() {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let ix = x.floor().clamp(0.0, (self.width - 1) as f32) as usize;
        let iy = y.floor().clamp(0.0, (self.height - 1) as f32) as usize;
        self.pixel_at(ix, iy)
    }

    fn sample_linear(&self, x: f32, y: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 || self.floats.is_empty() {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let fx = (x - 0.5).clamp(0.0, (self.width.saturating_sub(1)) as f32);
        let fy = (y - 0.5).clamp(0.0, (self.height.saturating_sub(1)) as f32);
        let x0 = fx.floor() as usize;
        let y0 = fy.floor() as usize;
        let x1 = (x0 + 1).min(self.width as usize - 1);
        let y1 = (y0 + 1).min(self.height as usize - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;
        let c00 = self.pixel_at(x0, y0);
        let c10 = self.pixel_at(x1, y0);
        let c01 = self.pixel_at(x0, y1);
        let c11 = self.pixel_at(x1, y1);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let top = c00[i] * (1.0 - tx) + c10[i] * tx;
            let bot = c01[i] * (1.0 - tx) + c11[i] * tx;
            out[i] = top * (1.0 - ty) + bot * ty;
        }
        out
    }

    fn pixel_at(&self, x: usize, y: usize) -> [f32; 4] {
        let ch = self.channels.clamp(1, 4) as usize;
        let base = (y * self.width as usize + x) * ch;
        let mut out = [0.0, 0.0, 0.0, 1.0];
        for (i, dst) in out.iter_mut().enumerate().take(ch) {
            *dst = self.floats.get(base + i).copied().unwrap_or(0.0);
        }
        out
    }
}

fn pbj_mask_indices(mask: u8) -> Vec<usize> {
    // Pixel Bender register masks encode components MSB-first: 0x8 = R/X, 0x4 = G/Y, 0x2 = B/Z, 0x1 = A/W.
    let mut out = Vec::with_capacity(4);
    for (bit, comp) in [(0x08u8, 0usize), (0x04, 1), (0x02, 2), (0x01, 3)] {
        if (mask & bit) != 0 {
            out.push(comp);
        }
    }
    if out.is_empty() {
        vec![0, 1, 2, 3]
    } else {
        out
    }
}

fn write_pbj_reg(regs: &mut [[f32; 4]], reg: u16, mask: u8, vals: &[f32]) {
    let idx = reg as usize;
    if idx >= regs.len() {
        return;
    }
    let comps = pbj_mask_indices(mask);
    for (vi, &c) in comps.iter().enumerate() {
        if let Some(&v) = vals.get(vi).or_else(|| vals.last()) {
            regs[idx][c] = v;
        }
    }
}

fn read_pbj_reg_masked(regs: &[[f32; 4]], reg: u16, mask: u8) -> [f32; 4] {
    let idx = reg as usize;
    if idx >= regs.len() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let comps = pbj_mask_indices(mask);
    let mut out = [0.0, 0.0, 0.0, 1.0];
    for (i, &c) in comps.iter().take(4).enumerate() {
        out[i] = regs[idx][c];
    }
    out
}

pub fn evaluate_shader(req: &ShaderEvalRequest) -> Result<ShaderEvalResult, Fte3dError> {
    validate_shader_precision(&req.precision_hint)?;
    if req.width == 0 || req.height == 0 {
        return Err(Fte3dError::Argument(
            "ShaderJob width and height must be positive.".into(),
        ));
    }
    let pixel_count = (req.width as usize).saturating_mul(req.height as usize);
    if pixel_count > MAX_SHADER_PIXELS {
        return Err(Fte3dError::Argument(
            "ShaderJob dimensions exceed maximum pixel budget.".into(),
        ));
    }
    let bc = BASE64
        .decode(req.bytecode_base64.as_bytes())
        .map_err(|e| Fte3dError::Argument(format!("Invalid base64 Shader byteCode: {e}")))?;
    let parsed = parse_pbj_or_json(&bc)?;

    // Decode and validate required inputs.
    let mut decoded_inputs: Vec<DecodedShaderInput> = Vec::with_capacity(parsed.meta.inputs.len());
    for meta_in in &parsed.meta.inputs {
        let supplied = req
            .inputs
            .iter()
            .find(|i| i.name == meta_in.name)
            .ok_or_else(|| {
                Fte3dError::Argument(format!(
                    "ShaderInput '{}' must have an input assigned before starting ShaderJob.",
                    meta_in.name
                ))
            })?;
        if supplied.width == 0 || supplied.height == 0 {
            return Err(Fte3dError::Argument(format!(
                "ShaderInput '{}' must have positive width and height.",
                meta_in.name
            )));
        }
        let floats = if let Some(b64) = &supplied.rgba_base64 {
            let bytes = BASE64.decode(b64.as_bytes()).map_err(|e| {
                Fte3dError::Argument(format!(
                    "Invalid base64 RGBA for ShaderInput '{}': {e}",
                    meta_in.name
                ))
            })?;
            let expected = (supplied.width as usize) * (supplied.height as usize) * 4;
            if bytes.len() < expected {
                return Err(Fte3dError::Argument(format!(
                    "ShaderInput '{}' buffer has insufficient pixel data.",
                    meta_in.name
                )));
            }
            let ch = meta_in.channels.clamp(1, 4) as usize;
            let mut f =
                Vec::with_capacity((supplied.width as usize) * (supplied.height as usize) * ch);
            for px in bytes[..expected].chunks_exact(4) {
                let rgba = [
                    px[0] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[2] as f32 / 255.0,
                    px[3] as f32 / 255.0,
                ];
                f.extend_from_slice(&rgba[..ch]);
            }
            f
        } else {
            let ch = supplied.channels.max(meta_in.channels).clamp(1, 4) as usize;
            let needed = (supplied.width as usize) * (supplied.height as usize) * ch;
            if supplied.floats.len() < needed {
                return Err(Fte3dError::Argument(format!(
                    "ShaderInput '{}' float buffer is shorter than width*height*channels ({} < {}).",
                    meta_in.name,
                    supplied.floats.len(),
                    needed
                )));
            }
            supplied.floats[..needed].to_vec()
        };
        decoded_inputs.push(DecodedShaderInput {
            width: supplied.width,
            height: supplied.height,
            channels: if supplied.rgba_base64.is_some() {
                meta_in.channels.clamp(1, 4)
            } else {
                supplied.channels.max(meta_in.channels).clamp(1, 4)
            },
            floats,
        });
    }

    // Resolve parameter values (supplied override defaultValue).
    let mut param_map: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for p in &parsed.meta.parameters {
        let comp_len = shader_param_component_count(&p.param_type)?;
        let mut vals: Vec<f32> = if let Some(v) = req.parameters.get(&p.name) {
            v.iter().map(|&x| x as f32).collect()
        } else {
            p.default_value.iter().map(|&x| x as f32).collect()
        };
        while vals.len() < comp_len {
            vals.push(0.0);
        }
        vals.truncate(comp_len);
        param_map.insert(p.name.clone(), vals);
    }

    let out_ch = parsed.out_channels.clamp(1, 4) as usize;
    let mut out_floats = Vec::with_capacity(pixel_count * out_ch);
    let mut out_rgba = Vec::with_capacity(pixel_count * 4);

    let max_reg = parsed
        .meta
        .parameters
        .iter()
        .map(|p| p.reg as usize + 4)
        .chain(std::iter::once(parsed.out_coord_reg as usize + 1))
        .chain(std::iter::once(parsed.out_pixel_reg as usize + 1))
        .chain(parsed.code.iter().map(|inst| match inst {
            PbjInst::LoadConst { dst_reg, .. } => *dst_reg as usize + 1,
            PbjInst::Sample {
                dst_reg, coord_reg, ..
            } => (*dst_reg as usize).max(*coord_reg as usize) + 1,
            PbjInst::Alu {
                dst_reg, src_reg, ..
            } => (*dst_reg as usize).max(*src_reg as usize) + 1,
            PbjInst::If { src_reg, .. } => *src_reg as usize + 1,
            PbjInst::Else | PbjInst::EndIf => 0,
        }))
        .max()
        .unwrap_or(16)
        .clamp(16, 512);

    let fast = req.precision_hint == "fast";

    for y in 0..req.height {
        for x in 0..req.width {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let px_out = if !parsed.code.is_empty() {
                let mut regs = vec![[0.0f32; 4]; max_reg];
                write_pbj_reg(
                    &mut regs,
                    parsed.out_coord_reg,
                    parsed.out_coord_mask,
                    &[fx, fy],
                );
                for p in &parsed.meta.parameters {
                    if let Some(vals) = param_map.get(&p.name) {
                        if vals.len() <= 4 {
                            write_pbj_reg(&mut regs, p.reg, p.mask, vals);
                        } else {
                            for (row, chunk) in vals.chunks(4).enumerate() {
                                write_pbj_reg(&mut regs, p.reg + row as u16, 0x0F, chunk);
                            }
                        }
                    }
                }
                // Condition stack: (parent_active, branch_cond)
                let mut cond_stack: Vec<(bool, bool)> = Vec::new();
                for inst in &parsed.code {
                    let active = cond_stack.last().is_none_or(|&(p, c)| p && c);
                    match *inst {
                        PbjInst::If { src_reg, src_swz } => {
                            let cond = if active {
                                let s_raw = regs.get(src_reg as usize).copied().unwrap_or([0.0; 4]);
                                s_raw[src_swz[0] as usize].abs() > 1e-6
                            } else {
                                false
                            };
                            cond_stack.push((active, cond));
                        }
                        PbjInst::Else => {
                            if let Some((parent_act, cur_cond)) = cond_stack.last_mut() {
                                *cur_cond = *parent_act && !*cur_cond;
                            }
                        }
                        PbjInst::EndIf => {
                            cond_stack.pop();
                        }
                        PbjInst::LoadConst {
                            dst_reg,
                            dst_mask,
                            value,
                        } => {
                            if active {
                                write_pbj_reg(&mut regs, dst_reg, dst_mask, &[value]);
                            }
                        }
                        PbjInst::Sample {
                            dst_reg,
                            dst_mask,
                            coord_reg,
                            coord_swz,
                            tex_index,
                            linear,
                        } => {
                            if !active {
                                continue;
                            }
                            let c_reg = regs
                                .get(coord_reg as usize)
                                .copied()
                                .unwrap_or([fx, fy, 0.0, 0.0]);
                            let sx = c_reg[coord_swz[0] as usize];
                            let sy = c_reg[coord_swz[1] as usize];
                            let sample = if let Some(inp) = decoded_inputs.get(tex_index as usize) {
                                if linear && !fast {
                                    inp.sample_linear(sx, sy)
                                } else {
                                    inp.sample_nearest(sx, sy)
                                }
                            } else {
                                [0.0, 0.0, 0.0, 0.0]
                            };
                            let comps = pbj_mask_indices(dst_mask);
                            let mut picked = Vec::with_capacity(comps.len());
                            for (i, _) in comps.iter().enumerate() {
                                picked.push(sample[i.min(3)]);
                            }
                            write_pbj_reg(&mut regs, dst_reg, dst_mask, &picked);
                        }
                        PbjInst::Alu {
                            op,
                            dst_reg,
                            dst_mask,
                            dst_len,
                            src_reg,
                            src_swz,
                        } => {
                            if !active {
                                continue;
                            }
                            let d_idx = dst_reg as usize;
                            let s_idx = src_reg as usize;
                            if d_idx >= regs.len() || s_idx >= regs.len() {
                                continue;
                            }
                            let dst_comps = pbj_mask_indices(dst_mask);
                            let s_raw = regs[s_idx];
                            let s = [
                                s_raw[src_swz[0] as usize],
                                s_raw[src_swz[1] as usize],
                                s_raw[src_swz[2] as usize],
                                s_raw[src_swz[3] as usize],
                            ];
                            let d_cur: Vec<f32> =
                                dst_comps.iter().map(|&c| regs[d_idx][c]).collect();
                            let n = (dst_len as usize).min(dst_comps.len()).max(1);
                            // Vector reduction & cross/normalize opcodes
                            if matches!(op, 0x26 | 0x27 | 0x37..=0x3D) {
                                let vec_len = (dst_len as usize).clamp(1, 4);
                                let d_full = regs[d_idx];
                                let res_vec: Vec<f32> = match op {
                                    0x26 => {
                                        // any(src)
                                        let any_true = (0..vec_len).any(|i| s[i].abs() > 1e-6);
                                        vec![if any_true { 1.0 } else { 0.0 }]
                                    }
                                    0x27 => {
                                        // all(src)
                                        let all_true = (0..vec_len).all(|i| s[i].abs() > 1e-6);
                                        vec![if all_true { 1.0 } else { 0.0 }]
                                    }
                                    0x38 => {
                                        // dot(dst, src)
                                        let mut acc = 0.0f32;
                                        for i in 0..vec_len {
                                            let a = d_cur.get(i).copied().unwrap_or(d_full[i]);
                                            acc += a * s[i];
                                        }
                                        vec![acc]
                                    }
                                    0x39 => {
                                        // cross(dst, src)
                                        let a0 = d_cur.first().copied().unwrap_or(d_full[0]);
                                        let a1 = d_cur.get(1).copied().unwrap_or(d_full[1]);
                                        let a2 = d_cur.get(2).copied().unwrap_or(d_full[2]);
                                        vec![
                                            a1 * s[2] - a2 * s[1],
                                            a2 * s[0] - a0 * s[2],
                                            a0 * s[1] - a1 * s[0],
                                        ]
                                    }
                                    0x3A => {
                                        // length(src)
                                        let sq: f32 = (0..vec_len).map(|i| s[i] * s[i]).sum();
                                        vec![sq.sqrt()]
                                    }
                                    0x3B => {
                                        // distance(dst, src)
                                        let mut sq = 0.0f32;
                                        for i in 0..vec_len {
                                            let a = d_cur.get(i).copied().unwrap_or(d_full[i]);
                                            let diff = a - s[i];
                                            sq += diff * diff;
                                        }
                                        vec![sq.sqrt()]
                                    }
                                    0x3C => {
                                        // normalize(src)
                                        let sq: f32 = (0..vec_len).map(|i| s[i] * s[i]).sum();
                                        let len = sq.sqrt().max(1e-12);
                                        (0..n).map(|i| s[i.min(3)] / len).collect()
                                    }
                                    _ => (0..n).map(|i| s[i.min(3)]).collect(),
                                };
                                write_pbj_reg(&mut regs, dst_reg, dst_mask, &res_vec);
                                continue;
                            }
                            let mut res = d_cur.clone();
                            for i in 0..n {
                                let a = d_cur[i];
                                let b = s[i.min(3)];
                                res[i] = match op {
                                    0x01 => a + b,
                                    0x02 => a - b,
                                    0x03 => a * b,
                                    0x04 => {
                                        if b.abs() > 1e-12 {
                                            1.0 / b
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x05 => {
                                        if b.abs() > 1e-12 {
                                            a / b
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x06 => a.atan2(b),
                                    0x07 => a.max(0.0).powf(b),
                                    0x08 => {
                                        if b.abs() > 1e-12 {
                                            a % b
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x09 => a.min(b),
                                    0x0A => a.max(b),
                                    0x0B => {
                                        if b < a {
                                            0.0
                                        } else {
                                            1.0
                                        }
                                    }
                                    0x0C => b.sin(),
                                    0x0D => b.cos(),
                                    0x0E => b.tan(),
                                    0x0F => b.clamp(-1.0, 1.0).asin(),
                                    0x10 => b.clamp(-1.0, 1.0).acos(),
                                    0x11 => b.atan(),
                                    0x12 => b.exp(),
                                    0x13 => b.exp2(),
                                    0x14 => b.max(1e-12).ln(),
                                    0x15 => b.max(1e-12).log2(),
                                    0x16 => b.max(0.0).sqrt(),
                                    0x17 => {
                                        let r = b.max(1e-12).sqrt();
                                        1.0 / r
                                    }
                                    0x18 => b.abs(),
                                    0x19 => {
                                        if b > 0.0 {
                                            1.0
                                        } else if b < 0.0 {
                                            -1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x1A => b.floor(),
                                    0x1B => b.ceil(),
                                    0x1C => b - b.floor(),
                                    0x1D => b,
                                    0x1E => b.trunc(),
                                    0x1F => b,
                                    0x20 => -b,
                                    0x21..=0x23 => {
                                        if b.abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x24 | 0x28 => {
                                        if (a - b).abs() <= 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x25 | 0x29 => {
                                        if (a - b).abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2A => {
                                        if a < b {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2B => {
                                        if a <= b {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2C => {
                                        if b.abs() <= 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2D => {
                                        if a.abs() > 1e-6 && b.abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2E => {
                                        if a.abs() > 1e-6 || b.abs() > 1e-6 {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    0x2F => {
                                        if (a.abs() > 1e-6) != (b.abs() > 1e-6) {
                                            1.0
                                        } else {
                                            0.0
                                        }
                                    }
                                    _ => b,
                                };
                            }
                            write_pbj_reg(&mut regs, dst_reg, dst_mask, &res);
                        }
                    }
                }
                read_pbj_reg_masked(&regs, parsed.out_pixel_reg, parsed.out_pixel_mask)
            } else {
                evaluate_portable_shader_pixel(
                    &parsed.meta.op,
                    fx,
                    fy,
                    req.width as f32,
                    req.height as f32,
                    &decoded_inputs,
                    &param_map,
                )
            };

            for &v in px_out.iter().take(out_ch) {
                out_floats.push(if v.is_finite() { v } else { 0.0 });
            }
            let rgba = match out_ch {
                1 => [px_out[0], px_out[0], px_out[0], 1.0],
                2 => [px_out[0], px_out[1], 0.0, 1.0],
                3 => [px_out[0], px_out[1], px_out[2], 1.0],
                _ => px_out,
            };
            for c in rgba {
                let cl = if c.is_finite() {
                    c.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                out_rgba.push((cl * 255.0).round() as u8);
            }
        }
    }

    Ok(ShaderEvalResult {
        width: req.width,
        height: req.height,
        channels: out_ch as u32,
        floats: out_floats,
        rgba_base64: BASE64.encode(&out_rgba),
    })
}

fn evaluate_portable_shader_pixel(
    op: &str,
    fx: f32,
    fy: f32,
    width: f32,
    height: f32,
    inputs: &[DecodedShaderInput],
    params: &BTreeMap<String, Vec<f32>>,
) -> [f32; 4] {
    let s0 = inputs.first().map(|i| i.sample_nearest(fx, fy)).unwrap_or([
        fx / width.max(1.0),
        fy / height.max(1.0),
        0.5,
        1.0,
    ]);
    let s1 = inputs.get(1).map(|i| i.sample_nearest(fx, fy));
    let get_p = |key: &str, def: &[f32]| -> Vec<f32> {
        params.get(key).cloned().unwrap_or_else(|| def.to_vec())
    };

    match op {
        "invert" => {
            let amt = get_p("amount", &[1.0])[0].clamp(0.0, 1.0);
            [
                s0[0] * (1.0 - amt) + (1.0 - s0[0]) * amt,
                s0[1] * (1.0 - amt) + (1.0 - s0[1]) * amt,
                s0[2] * (1.0 - amt) + (1.0 - s0[2]) * amt,
                s0[3],
            ]
        }
        "tint" | "multiply" => {
            let c = params
                .get("color")
                .or_else(|| params.get("tint"))
                .or_else(|| params.get("factor"))
                .cloned()
                .unwrap_or_else(|| vec![1.0, 1.0, 1.0, 1.0]);
            let r = c.first().copied().unwrap_or(1.0);
            let g = c.get(1).copied().unwrap_or(r);
            let b = c.get(2).copied().unwrap_or(r);
            let a = c.get(3).copied().unwrap_or(1.0);
            if let Some(other) = s1 {
                [
                    s0[0] * other[0] * r,
                    s0[1] * other[1] * g,
                    s0[2] * other[2] * b,
                    s0[3] * other[3] * a,
                ]
            } else {
                [s0[0] * r, s0[1] * g, s0[2] * b, s0[3] * a]
            }
        }
        "add" | "brightness" => {
            let off = params
                .get("offset")
                .or_else(|| params.get("brightness"))
                .or_else(|| params.get("amount"))
                .cloned()
                .unwrap_or_else(|| vec![0.0]);
            let dr = off.first().copied().unwrap_or(0.0);
            let dg = off.get(1).copied().unwrap_or(dr);
            let db = off.get(2).copied().unwrap_or(dr);
            let da = off.get(3).copied().unwrap_or(0.0);
            if let Some(other) = s1 {
                [
                    s0[0] + other[0] + dr,
                    s0[1] + other[1] + dg,
                    s0[2] + other[2] + db,
                    (s0[3] + other[3] + da).clamp(0.0, 1.0),
                ]
            } else {
                [s0[0] + dr, s0[1] + dg, s0[2] + db, s0[3] + da]
            }
        }
        "mix" | "blend" => {
            let other = s1.unwrap_or([1.0 - s0[0], 1.0 - s0[1], 1.0 - s0[2], s0[3]]);
            let t = get_p("amount", &[0.5])[0].clamp(0.0, 1.0);
            [
                s0[0] * (1.0 - t) + other[0] * t,
                s0[1] * (1.0 - t) + other[1] * t,
                s0[2] * (1.0 - t) + other[2] * t,
                s0[3] * (1.0 - t) + other[3] * t,
            ]
        }
        "screen" => {
            let other = s1.unwrap_or([0.0, 0.0, 0.0, 1.0]);
            [
                1.0 - (1.0 - s0[0]) * (1.0 - other[0]),
                1.0 - (1.0 - s0[1]) * (1.0 - other[1]),
                1.0 - (1.0 - s0[2]) * (1.0 - other[2]),
                (s0[3] + other[3] - s0[3] * other[3]).clamp(0.0, 1.0),
            ]
        }
        "difference" => {
            let other = s1.unwrap_or([0.0, 0.0, 0.0, 1.0]);
            [
                (s0[0] - other[0]).abs(),
                (s0[1] - other[1]).abs(),
                (s0[2] - other[2]).abs(),
                s0[3].max(other[3]),
            ]
        }
        "subtract" => {
            let other = s1.unwrap_or([0.0, 0.0, 0.0, 0.0]);
            [
                (s0[0] - other[0]).clamp(0.0, 1.0),
                (s0[1] - other[1]).clamp(0.0, 1.0),
                (s0[2] - other[2]).clamp(0.0, 1.0),
                s0[3].max(other[3]),
            ]
        }
        "threshold" => {
            let th = get_p("threshold", &[0.5])[0];
            let lum = 0.2126 * s0[0] + 0.7152 * s0[1] + 0.0722 * s0[2];
            let v = if lum >= th { 1.0 } else { 0.0 };
            [v, v, v, s0[3]]
        }
        "gradient" => {
            let c0 = get_p("startColor", &[0.0, 0.0, 0.0, 1.0]);
            let c1 = get_p("endColor", &[1.0, 1.0, 1.0, 1.0]);
            let t = (fx / width.max(1.0)).clamp(0.0, 1.0);
            [
                c0.first().copied().unwrap_or(0.0) * (1.0 - t)
                    + c1.first().copied().unwrap_or(1.0) * t,
                c0.get(1).copied().unwrap_or(0.0) * (1.0 - t)
                    + c1.get(1).copied().unwrap_or(1.0) * t,
                c0.get(2).copied().unwrap_or(0.0) * (1.0 - t)
                    + c1.get(2).copied().unwrap_or(1.0) * t,
                c0.get(3).copied().unwrap_or(1.0) * (1.0 - t)
                    + c1.get(3).copied().unwrap_or(1.0) * t,
            ]
        }
        "solid" | "fill" => {
            let c = get_p("color", &[1.0, 0.0, 1.0, 1.0]);
            [
                c.first().copied().unwrap_or(1.0),
                c.get(1).copied().unwrap_or(0.0),
                c.get(2).copied().unwrap_or(1.0),
                c.get(3).copied().unwrap_or(1.0),
            ]
        }
        _ => {
            // Auto mode: if a 3/4-component color parameter exists, modulate s0; if a scalar amount/brightness exists, apply it; otherwise return s0.
            if let Some(c) = params.get("color").or_else(|| params.get("tint")) {
                return [
                    s0[0] * c.first().copied().unwrap_or(1.0),
                    s0[1] * c.get(1).copied().unwrap_or(1.0),
                    s0[2] * c.get(2).copied().unwrap_or(1.0),
                    s0[3] * c.get(3).copied().unwrap_or(1.0),
                ];
            }
            if let Some(m) = params.get("matrix")
                && m.len() == 16
            {
                return [
                    s0[0] * m[0] + s0[1] * m[1] + s0[2] * m[2] + s0[3] * m[3],
                    s0[0] * m[4] + s0[1] * m[5] + s0[2] * m[6] + s0[3] * m[7],
                    s0[0] * m[8] + s0[1] * m[9] + s0[2] * m[10] + s0[3] * m[11],
                    s0[0] * m[12] + s0[1] * m[13] + s0[2] * m[14] + s0[3] * m[15],
                ];
            }
            if let Some((_, first_param)) = params.iter().next()
                && inputs.is_empty()
            {
                let r = first_param.first().copied().unwrap_or(s0[0]);
                let g = first_param.get(1).copied().unwrap_or(r);
                let b = first_param.get(2).copied().unwrap_or(r);
                let a = first_param.get(3).copied().unwrap_or(1.0);
                return [r, g, b, a];
            }
            if let Some(other) = s1 {
                return [
                    (s0[0] + other[0]) * 0.5,
                    (s0[1] + other[1]) * 0.5,
                    (s0[2] + other[2]) * 0.5,
                    (s0[3] + other[3]) * 0.5,
                ];
            }
            s0
        }
    }
}

pub fn evaluate_shader_filter_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    bytecode_base64: &str,
    precision_hint: &str,
    params_json: &str,
) -> Vec<u8> {
    if width == 0 || height == 0 || bytecode_base64.is_empty() {
        return src.to_vec();
    }
    let Ok(bc) = BASE64.decode(bytecode_base64.as_bytes()) else {
        return src.to_vec();
    };
    let Ok(meta) = parse_shader_bytecode(&bc) else {
        return src.to_vec();
    };
    let raw_json: serde_json::Value =
        serde_json::from_str(params_json).unwrap_or(serde_json::Value::Null);
    let mut parameters: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut extra_inputs: BTreeMap<String, ShaderRuntimeInput> = BTreeMap::new();
    if let Some(obj) = raw_json.as_object() {
        for (k, v) in obj {
            if k == "__extra_inputs__" {
                if let Some(arr) = v.as_array() {
                    for item in arr {
                        if let Ok(inp) = serde_json::from_value::<ShaderRuntimeInput>(item.clone())
                        {
                            extra_inputs.insert(inp.name.clone(), inp);
                        }
                    }
                }
            } else if let Some(arr) = v.as_array() {
                let vals: Vec<f64> = arr.iter().filter_map(|x| x.as_f64()).collect();
                parameters.insert(k.clone(), vals);
            }
        }
    }
    let src_b64 = BASE64.encode(src);
    let mut inputs = Vec::new();
    if let Some(first_in) = meta.inputs.first() {
        inputs.push(ShaderRuntimeInput {
            name: first_in.name.clone(),
            width,
            height,
            channels: first_in.channels,
            floats: Vec::new(),
            rgba_base64: Some(src_b64),
        });
    }
    // Use pre-bound secondary ShaderInputs from __extra_inputs__ when provided, falling back to src.
    for extra in meta.inputs.iter().skip(1) {
        if let Some(supplied) = extra_inputs.remove(&extra.name) {
            inputs.push(supplied);
        } else {
            inputs.push(ShaderRuntimeInput {
                name: extra.name.clone(),
                width,
                height,
                channels: extra.channels,
                floats: Vec::new(),
                rgba_base64: Some(BASE64.encode(src)),
            });
        }
    }
    let req = ShaderEvalRequest {
        bytecode_base64: bytecode_base64.to_string(),
        precision_hint: if matches!(precision_hint, "full" | "fast") {
            precision_hint.to_string()
        } else {
            "full".into()
        },
        width,
        height,
        inputs,
        parameters,
    };
    match evaluate_shader(&req) {
        Ok(res) => BASE64
            .decode(res.rgba_base64.as_bytes())
            .unwrap_or_else(|_| src.to_vec()),
        Err(_) => src.to_vec(),
    }
}

pub fn evaluate_blend_shader_rgba(
    bg_rgba: &[u8],
    fg_rgba: &[u8],
    width: u32,
    height: u32,
    bytecode_base64: &str,
    precision_hint: &str,
    params_json: &str,
) -> Result<Vec<u8>, Fte3dError> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| Fte3dError::Range("Blend shader dimensions overflow.".into()))?;
    if pixel_count == 0 {
        return Ok(Vec::new());
    }
    if pixel_count > MAX_SHADER_PIXELS {
        return Err(Fte3dError::Range(
            "Blend shader dimensions exceed 4 Mi-pixel limit.".into(),
        ));
    }
    let expected_len = pixel_count * 4;
    if bg_rgba.len() != expected_len || fg_rgba.len() != expected_len {
        return Err(Fte3dError::Argument(
            "Blend shader bg/fg RGBA byte length mismatch.".into(),
        ));
    }
    if bytecode_base64.is_empty() {
        return Err(Fte3dError::Argument(
            "BlendMode.SHADER requires non-empty Shader byteCode.".into(),
        ));
    }
    let bc = BASE64
        .decode(bytecode_base64.as_bytes())
        .map_err(|e| Fte3dError::Argument(format!("Invalid blendShader base64 bytecode: {e}")))?;
    let meta = parse_shader_bytecode(&bc)?;

    let raw_json: serde_json::Value =
        serde_json::from_str(params_json).unwrap_or(serde_json::Value::Null);
    let mut parameters: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut extra_inputs: BTreeMap<String, ShaderRuntimeInput> = BTreeMap::new();
    if let Some(obj) = raw_json.as_object() {
        for (k, v) in obj {
            if k == "__extra_inputs__" {
                if let Some(arr) = v.as_array() {
                    for item in arr {
                        if let Ok(inp) = serde_json::from_value::<ShaderRuntimeInput>(item.clone())
                        {
                            extra_inputs.insert(inp.name.clone(), inp);
                        }
                    }
                }
            } else if let Some(arr) = v.as_array() {
                let vals: Vec<f64> = arr.iter().filter_map(|x| x.as_f64()).collect();
                parameters.insert(k.clone(), vals);
            }
        }
    }

    let bg_b64 = BASE64.encode(bg_rgba);
    let fg_b64 = BASE64.encode(fg_rgba);
    let mut inputs = Vec::with_capacity(meta.inputs.len());
    if meta.inputs.len() >= 2 {
        inputs.push(ShaderRuntimeInput {
            name: meta.inputs[0].name.clone(),
            width,
            height,
            channels: meta.inputs[0].channels,
            floats: Vec::new(),
            rgba_base64: Some(bg_b64),
        });
        inputs.push(ShaderRuntimeInput {
            name: meta.inputs[1].name.clone(),
            width,
            height,
            channels: meta.inputs[1].channels,
            floats: Vec::new(),
            rgba_base64: Some(fg_b64.clone()),
        });
        for extra in meta.inputs.iter().skip(2) {
            if let Some(supplied) = extra_inputs.remove(&extra.name) {
                inputs.push(supplied);
            } else {
                inputs.push(ShaderRuntimeInput {
                    name: extra.name.clone(),
                    width,
                    height,
                    channels: extra.channels,
                    floats: Vec::new(),
                    rgba_base64: Some(fg_b64.clone()),
                });
            }
        }
    } else if let Some(single) = meta.inputs.first() {
        inputs.push(ShaderRuntimeInput {
            name: single.name.clone(),
            width,
            height,
            channels: single.channels,
            floats: Vec::new(),
            rgba_base64: Some(fg_b64),
        });
    }

    let req = ShaderEvalRequest {
        bytecode_base64: bytecode_base64.to_string(),
        precision_hint: if matches!(precision_hint, "full" | "fast") {
            precision_hint.to_string()
        } else {
            "full".into()
        },
        width,
        height,
        inputs,
        parameters,
    };
    let res = evaluate_shader(&req)?;
    let shader_rgba = BASE64
        .decode(res.rgba_base64.as_bytes())
        .map_err(|e| Fte3dError::Argument(format!("Invalid shader output base64: {e}")))?;
    if shader_rgba.len() != expected_len {
        return Err(Fte3dError::Argument(
            "Blend shader output length mismatch.".into(),
        ));
    }
    let mut out = bg_rgba.to_vec();
    for px in 0..pixel_count {
        let i = px * 4;
        if fg_rgba[i + 3] > 0 {
            out[i..i + 4].copy_from_slice(&shader_rgba[i..i + 4]);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwfHeaderInfo {
    pub swf_version: u32,
    pub action_script_version: u32,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub frame_count: u32,
    pub background_color: Option<u32>,
}

pub fn inspect_swf_bytes(bytes: &[u8]) -> Result<SwfHeaderInfo, Fte3dError> {
    if bytes.len() < 8 {
        return Err(Fte3dError::Argument(
            "SWF header is shorter than 8 bytes.".into(),
        ));
    }
    let sig = &bytes[0..3];
    if sig != b"FWS" && sig != b"CWS" && sig != b"ZWS" {
        return Err(Fte3dError::Argument(
            "Not a valid SWF header signature.".into(),
        ));
    }
    let swf_version = bytes[3] as u32;
    let _file_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let body: Vec<u8> = if sig == b"FWS" {
        bytes[8..].to_vec()
    } else if sig == b"CWS" {
        use std::io::Read;
        const MAX_CWS_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
        let decoder = flate2::read::ZlibDecoder::new(&bytes[8..]);
        let mut out = Vec::new();
        decoder
            .take((MAX_CWS_DECOMPRESSED_BYTES as u64) + 1)
            .read_to_end(&mut out)
            .map_err(|e| Fte3dError::Argument(format!("Invalid CWS zlib stream: {e}")))?;
        if out.len() > MAX_CWS_DECOMPRESSED_BYTES {
            return Err(Fte3dError::Argument(
                "Decompressed CWS SWF payload exceeds 16 MiB limit.".into(),
            ));
        }
        out
    } else {
        const MAX_ZWS_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
        if bytes.len() < 17 {
            return Err(Fte3dError::Argument(
                "Truncated ZWS LZMA header (requires at least 17 bytes).".into(),
            ));
        }
        let uncompressed_body_len = _file_len.saturating_sub(8) as u64;
        if uncompressed_body_len as usize > MAX_ZWS_DECOMPRESSED_BYTES {
            return Err(Fte3dError::Argument(
                "Decompressed ZWS SWF payload exceeds 16 MiB limit.".into(),
            ));
        }
        let mut lzma_stream = Vec::with_capacity(13 + (bytes.len() - 17));
        lzma_stream.extend_from_slice(&bytes[12..17]);
        lzma_stream.extend_from_slice(&uncompressed_body_len.to_le_bytes());
        lzma_stream.extend_from_slice(&bytes[17..]);
        crate::as_binary::binary_transform(
            "uncompress",
            &lzma_stream,
            "",
            "lzma",
            MAX_ZWS_DECOMPRESSED_BYTES,
        )
        .map_err(|e| Fte3dError::Argument(format!("Invalid ZWS LZMA stream: {e}")))?
    };

    if body.len() < 5 {
        return Err(Fte3dError::Argument("Truncated SWF RECT header.".into()));
    }
    let nbits = (body[0] >> 3) as usize;
    let total_rect_bits = 5 + nbits * 4;
    let rect_bytes = total_rect_bits.div_ceil(8);
    if body.len() < rect_bytes + 4 {
        return Err(Fte3dError::Argument(
            "Truncated SWF frame rate / count header.".into(),
        ));
    }
    let read_sbits = |start_bit: usize, count: usize| -> i64 {
        if count == 0 {
            return 0;
        }
        let mut val = 0u64;
        for i in 0..count {
            let bit_idx = start_bit + i;
            let b = body[bit_idx / 8];
            let bit = (b >> (7 - (bit_idx % 8))) & 1;
            val = (val << 1) | (bit as u64);
        }
        let sign_bit = 1u64 << (count - 1);
        if (val & sign_bit) != 0 {
            (val | (!0u64 << count)) as i64
        } else {
            val as i64
        }
    };
    let xmin = read_sbits(5, nbits);
    let xmax = read_sbits(5 + nbits, nbits);
    let ymin = read_sbits(5 + nbits * 2, nbits);
    let ymax = read_sbits(5 + nbits * 3, nbits);
    let width = (((xmax - xmin).max(20) / 20) as u64).min(u32::MAX as u64) as u32;
    let height = (((ymax - ymin).max(20) / 20) as u64).min(u32::MAX as u64) as u32;

    let fr_raw = u16::from_le_bytes([body[rect_bytes], body[rect_bytes + 1]]);
    let frame_rate = ((fr_raw as f64) / 256.0).max(1.0);
    let frame_count =
        u16::from_le_bytes([body[rect_bytes + 2], body[rect_bytes + 3]]).max(1) as u32;

    let mut pos = rect_bytes + 4;
    let mut action_script_version = if swf_version >= 9 { 3u32 } else { 2u32 };
    let mut saw_file_attributes = false;
    let mut background_color = None;

    while pos + 2 <= body.len() {
        let tag_code_and_len = u16::from_le_bytes([body[pos], body[pos + 1]]);
        pos += 2;
        let tag_code = tag_code_and_len >> 6;
        let mut tag_len = (tag_code_and_len & 0x3F) as usize;
        if tag_len == 0x3F {
            if pos + 4 > body.len() {
                break;
            }
            tag_len = u32::from_le_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]])
                as usize;
            pos += 4;
        }
        if pos + tag_len > body.len() {
            break;
        }
        if tag_code == 0 {
            break;
        }
        if tag_code == 69 && tag_len >= 1 {
            // FileAttributes tag: bit 3 (0x08) is ActionScript3
            saw_file_attributes = true;
            let flags = body[pos];
            action_script_version = if (flags & 0x08) != 0 { 3 } else { 2 };
        } else if tag_code == 9 && tag_len >= 3 {
            // SetBackgroundColor tag
            background_color = Some(
                ((body[pos] as u32) << 16) | ((body[pos + 1] as u32) << 8) | (body[pos + 2] as u32),
            );
        }
        pos += tag_len;
    }
    if !saw_file_attributes && swf_version < 9 {
        action_script_version = 2;
    }

    Ok(SwfHeaderInfo {
        swf_version,
        action_script_version,
        width,
        height,
        frame_rate,
        frame_count,
        background_color,
    })
}
