#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float,
    Double,
    Bool,
    Char,
    String,
    Struct,
    Pointer,
    Array,
    Unknown,
}

impl TypeKind {
    pub fn c_enum_str(&self) -> &'static str {
        match self {
            TypeKind::Int8 => "TYPE_INT8",
            TypeKind::Int16 => "TYPE_INT16",
            TypeKind::Int32 => "TYPE_INT32",
            TypeKind::Int64 => "TYPE_INT64",
            TypeKind::UInt8 => "TYPE_UINT8",
            TypeKind::UInt16 => "TYPE_UINT16",
            TypeKind::UInt32 => "TYPE_UINT32",
            TypeKind::UInt64 => "TYPE_UINT64",
            TypeKind::Float => "TYPE_FLOAT",
            TypeKind::Double => "TYPE_DOUBLE",
            TypeKind::Bool => "TYPE_BOOL",
            TypeKind::Char => "TYPE_CHAR",
            TypeKind::String => "TYPE_STRING",
            TypeKind::Struct => "TYPE_STRUCT",
            TypeKind::Pointer => "TYPE_POINTER",
            TypeKind::Array => "TYPE_ARRAY",
            TypeKind::Unknown => "TYPE_UNKNOWN",
        }
    }

    pub fn from_c_type(raw_type: &str, is_ptr: bool, is_arr: bool) -> Self {
        if is_ptr {
            if raw_type == "char" || raw_type == "const char" {
                return TypeKind::String;
            }
            return TypeKind::Pointer;
        }
        if is_arr {
            return TypeKind::Array;
        }

        let clean = raw_type.trim();
        match clean {
            "int8_t" | "signed char" | "i8" => TypeKind::Int8,
            "uint8_t" | "unsigned char" | "u8" => TypeKind::UInt8,
            "int16_t" | "short" | "short int" | "i16" => TypeKind::Int16,
            "uint16_t" | "unsigned short" | "u16" => TypeKind::UInt16,
            "int" | "int32_t" | "long" | "signed int" | "i32" => TypeKind::Int32,
            "uint32_t" | "unsigned int" | "unsigned long" | "u32" => TypeKind::UInt32,
            "int64_t" | "long long" | "i64" | "isize" => TypeKind::Int64,
            "uint64_t" | "unsigned long long" | "size_t" | "u64" | "usize" => TypeKind::UInt64,
            "float" | "f32" => TypeKind::Float,
            "double" | "f64" => TypeKind::Double,
            "bool" | "_Bool" => TypeKind::Bool,
            "char" => TypeKind::Char,
            _ => {
                if clean.starts_with("struct ") {
                    TypeKind::Struct
                } else if clean.chars().next().map_or(false, |c| c.is_uppercase()) {
                    TypeKind::Struct
                } else {
                    TypeKind::Unknown
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CtAttr {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct CtField {
    pub name: String,
    pub type_name: String,
    pub kind: TypeKind,
    pub is_pointer: bool,
    pub array_len: usize,
    pub attrs: Vec<CtAttr>,
}

#[derive(Debug, Clone)]
pub struct CtStruct {
    pub name: String,
    pub fields: Vec<CtField>,
    pub attrs: Vec<CtAttr>,
}

#[derive(Debug, Clone)]
pub struct CtVariant {
    pub name: String,
    pub value: i64,
    pub attrs: Vec<CtAttr>,
}

#[derive(Debug, Clone)]
pub struct CtEnum {
    pub name: String,
    pub variants: Vec<CtVariant>,
    pub attrs: Vec<CtAttr>,
}
