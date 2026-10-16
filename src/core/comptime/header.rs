use super::types::{CtAttr, CtEnum, CtStruct};

fn escape_c(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

fn format_legacy_attr_summary(attrs: &[CtAttr]) -> String {
    if attrs.is_empty() {
        return String::new();
    }
    let first = &attrs[0];
    if first.value.is_empty() {
        escape_c(&first.name)
    } else {
        escape_c(&first.value)
    }
}

pub fn generate_comptime_header(structs: &[CtStruct], enums: &[CtEnum]) -> String {
    let mut s = String::with_capacity(20480);

    s.push_str(
        r#"#ifndef COMPTIME_H
#define COMPTIME_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdarg.h>
#include <ctype.h>

// ============================================================================
// NULL-Safe String Operations for Bulletproof Comptime Serializers (JSON/RON/TOML)
// ============================================================================

static inline size_t _ct_safe_strlen(const char* s) {
    return s ? strlen(s) : 0;
}

static inline int _ct_safe_strcmp(const char* a, const char* b) {
    if (a == b) return 0;
    if (!a) return -1;
    if (!b) return 1;
    return strcmp(a, b);
}

static inline int _ct_safe_strncmp(const char* a, const char* b, size_t n) {
    if (a == b || n == 0) return 0;
    if (!a) return -1;
    if (!b) return 1;
    return strncmp(a, b, n);
}

static inline bool ct_streq(const char* a, const char* b) {
    return _ct_safe_strcmp(a, b) == 0;
}

#ifdef __TINYC__
#define strlen(s) _ct_safe_strlen(s)
#define strcmp(a, b) _ct_safe_strcmp(a, b)
#define strncmp(a, b, n) _ct_safe_strncmp(a, b, n)
#endif

typedef enum {
    TYPE_INT8,
    TYPE_INT16,
    TYPE_INT32,
    TYPE_INT64,
    TYPE_UINT8,
    TYPE_UINT16,
    TYPE_UINT32,
    TYPE_UINT64,
    TYPE_FLOAT,
    TYPE_DOUBLE,
    TYPE_BOOL,
    TYPE_CHAR,
    TYPE_STRING,
    TYPE_STRUCT,
    TYPE_POINTER,
    TYPE_ARRAY,
    TYPE_UNKNOWN
} TypeKind;

typedef struct ct_attr {
    const char* name;
    const char* value;
} ct_attr;

typedef struct ct_field {
    const char* name;
    const char* type;
    TypeKind kind;
    int is_pointer;
    int array_len;
    const char* attr;
    int attr_count;
    const ct_attr* attrs;
} ct_field;

typedef struct ct_struct {
    const char* name;
    int field_count;
    const ct_field* fields;
    const char* attr;
    int attr_count;
    const ct_attr* attrs;
} ct_struct;

typedef struct ct_variant {
    const char* name;
    int64_t value;
    const char* attr;
    int attr_count;
    const ct_attr* attrs;
} ct_variant;

typedef struct ct_enum {
    const char* name;
    int variant_count;
    const ct_variant* variants;
    const char* attr;
    int attr_count;
    const ct_attr* attrs;
} ct_enum;

"#,
    );

    // Emit static sub-arrays for struct and field metadata (unlimited capacity, zero stack/array overflow)
    for (si, st) in structs.iter().enumerate() {
        if !st.attrs.is_empty() {
            s.push_str(&format!("static const ct_attr _ct_s{}_attrs[] = {{\n", si));
            for a in &st.attrs {
                s.push_str(&format!(
                    "    {{ .name = \"{}\", .value = \"{}\" }},\n",
                    escape_c(&a.name),
                    escape_c(&a.value)
                ));
            }
            s.push_str("};\n");
        }

        for (fi, f) in st.fields.iter().enumerate() {
            if !f.attrs.is_empty() {
                s.push_str(&format!(
                    "static const ct_attr _ct_s{}_f{}_attrs[] = {{\n",
                    si, fi
                ));
                for a in &f.attrs {
                    s.push_str(&format!(
                        "    {{ .name = \"{}\", .value = \"{}\" }},\n",
                        escape_c(&a.name),
                        escape_c(&a.value)
                    ));
                }
                s.push_str("};\n");
            }
        }

        if !st.fields.is_empty() {
            s.push_str(&format!(
                "static const ct_field _ct_s{}_fields[] = {{\n",
                si
            ));
            for (fi, f) in st.fields.iter().enumerate() {
                let f_attrs_ptr = if f.attrs.is_empty() {
                    "NULL".to_string()
                } else {
                    format!("_ct_s{}_f{}_attrs", si, fi)
                };
                s.push_str(&format!(
                    "    {{ .name = \"{}\", .type = \"{}\", .kind = {}, .is_pointer = {}, .array_len = {}, .attr = \"{}\", .attr_count = {}, .attrs = {} }},\n",
                    escape_c(&f.name),
                    escape_c(&f.type_name),
                    f.kind.c_enum_str(),
                    if f.is_pointer { 1 } else { 0 },
                    f.array_len,
                    format_legacy_attr_summary(&f.attrs),
                    f.attrs.len(),
                    f_attrs_ptr,
                ));
            }
            s.push_str("};\n");
        }
    }

    s.push_str("\nstatic const ct_struct _CT_ALL_STRUCTS[] = {\n");
    for (si, st) in structs.iter().enumerate() {
        let fields_ptr = if st.fields.is_empty() {
            "NULL".to_string()
        } else {
            format!("_ct_s{}_fields", si)
        };
        let attrs_ptr = if st.attrs.is_empty() {
            "NULL".to_string()
        } else {
            format!("_ct_s{}_attrs", si)
        };
        s.push_str(&format!(
            "    {{ .name = \"{}\", .field_count = {}, .fields = {}, .attr = \"{}\", .attr_count = {}, .attrs = {} }},\n",
            escape_c(&st.name),
            st.fields.len(),
            fields_ptr,
            format_legacy_attr_summary(&st.attrs),
            st.attrs.len(),
            attrs_ptr,
        ));
    }
    // Always include a sentinel element so _CT_ALL_STRUCTS is never a zero-length C array
    s.push_str("    { .name = NULL, .field_count = 0, .fields = NULL, .attr = \"\", .attr_count = 0, .attrs = NULL }\n};\n");
    s.push_str(&format!(
        "static const int _CT_STRUCT_COUNT = {};\n\n",
        structs.len()
    ));

    // Emit static sub-arrays for enum and variant metadata
    for (ei, en) in enums.iter().enumerate() {
        if !en.attrs.is_empty() {
            s.push_str(&format!("static const ct_attr _ct_e{}_attrs[] = {{\n", ei));
            for a in &en.attrs {
                s.push_str(&format!(
                    "    {{ .name = \"{}\", .value = \"{}\" }},\n",
                    escape_c(&a.name),
                    escape_c(&a.value)
                ));
            }
            s.push_str("};\n");
        }

        for (vi, v) in en.variants.iter().enumerate() {
            if !v.attrs.is_empty() {
                s.push_str(&format!(
                    "static const ct_attr _ct_e{}_v{}_attrs[] = {{\n",
                    ei, vi
                ));
                for a in &v.attrs {
                    s.push_str(&format!(
                        "    {{ .name = \"{}\", .value = \"{}\" }},\n",
                        escape_c(&a.name),
                        escape_c(&a.value)
                    ));
                }
                s.push_str("};\n");
            }
        }

        if !en.variants.is_empty() {
            s.push_str(&format!(
                "static const ct_variant _ct_e{}_variants[] = {{\n",
                ei
            ));
            for (vi, v) in en.variants.iter().enumerate() {
                let v_attrs_ptr = if v.attrs.is_empty() {
                    "NULL".to_string()
                } else {
                    format!("_ct_e{}_v{}_attrs", ei, vi)
                };
                s.push_str(&format!(
                    "    {{ .name = \"{}\", .value = {}LL, .attr = \"{}\", .attr_count = {}, .attrs = {} }},\n",
                    escape_c(&v.name),
                    v.value,
                    format_legacy_attr_summary(&v.attrs),
                    v.attrs.len(),
                    v_attrs_ptr,
                ));
            }
            s.push_str("};\n");
        }
    }

    s.push_str("\nstatic const ct_enum _CT_ALL_ENUMS[] = {\n");
    for (ei, en) in enums.iter().enumerate() {
        let variants_ptr = if en.variants.is_empty() {
            "NULL".to_string()
        } else {
            format!("_ct_e{}_variants", ei)
        };
        let attrs_ptr = if en.attrs.is_empty() {
            "NULL".to_string()
        } else {
            format!("_ct_e{}_attrs", ei)
        };
        s.push_str(&format!(
            "    {{ .name = \"{}\", .variant_count = {}, .variants = {}, .attr = \"{}\", .attr_count = {}, .attrs = {} }},\n",
            escape_c(&en.name),
            en.variants.len(),
            variants_ptr,
            format_legacy_attr_summary(&en.attrs),
            en.attrs.len(),
            attrs_ptr,
        ));
    }
    s.push_str("    { .name = NULL, .variant_count = 0, .variants = NULL, .attr = \"\", .attr_count = 0, .attrs = NULL }\n};\n");
    s.push_str(&format!(
        "static const int _CT_ENUM_COUNT = {};\n\n",
        enums.len()
    ));

    // Functions and Macros
    s.push_str(
        r#"
// ============================================================================
// Direct Array Access & Query API
// ============================================================================

static inline int struct_count(void) {
    return _CT_STRUCT_COUNT;
}

static inline const ct_struct* get_struct(int index) {
    if (index >= 0 && index < _CT_STRUCT_COUNT) {
        return &_CT_ALL_STRUCTS[index];
    }
    return NULL;
}

static inline const ct_struct* find_struct(const char* name) {
    if (!name) return NULL;
    for (int i = 0; i < _CT_STRUCT_COUNT; i++) {
        if (_CT_ALL_STRUCTS[i].name && _ct_safe_strcmp(_CT_ALL_STRUCTS[i].name, name) == 0) {
            return &_CT_ALL_STRUCTS[i];
        }
    }
    return NULL;
}

static inline const ct_field* find_field(const ct_struct* s, const char* name) {
    if (!s || !s->fields || !name) return NULL;
    for (int i = 0; i < s->field_count; i++) {
        if (s->fields[i].name && _ct_safe_strcmp(s->fields[i].name, name) == 0) {
            return &s->fields[i];
        }
    }
    return NULL;
}

static inline int enum_count(void) {
    return _CT_ENUM_COUNT;
}

static inline const ct_enum* get_enum(int index) {
    if (index >= 0 && index < _CT_ENUM_COUNT) {
        return &_CT_ALL_ENUMS[index];
    }
    return NULL;
}

static inline const ct_enum* find_enum(const char* name) {
    if (!name) return NULL;
    for (int i = 0; i < _CT_ENUM_COUNT; i++) {
        if (_CT_ALL_ENUMS[i].name && _ct_safe_strcmp(_CT_ALL_ENUMS[i].name, name) == 0) {
            return &_CT_ALL_ENUMS[i];
        }
    }
    return NULL;
}

static inline const ct_variant* find_variant(const ct_enum* e, const char* name) {
    if (!e || !e->variants || !name) return NULL;
    for (int i = 0; i < e->variant_count; i++) {
        if (e->variants[i].name && _ct_safe_strcmp(e->variants[i].name, name) == 0) {
            return &e->variants[i];
        }
    }
    return NULL;
}

// ============================================================================
// NULL-Safe Attribute Inspection Helpers (with smart 'def' <-> 'default' alias)
// ============================================================================

static const char* _ct_last_queried_struct_attr = NULL;
static int _ct_queried_struct_attr_matches = 0;

static inline bool _ct_attr_name_matches(const char* actual, const char* queried) {
    if (!actual || !queried) return false;
    if (_ct_safe_strcmp(actual, queried) == 0) return true;
    if ((_ct_safe_strcmp(actual, "default") == 0 && _ct_safe_strcmp(queried, "def") == 0) ||
        (_ct_safe_strcmp(actual, "def") == 0 && _ct_safe_strcmp(queried, "default") == 0)) {
        return true;
    }
    return false;
}

static inline bool ct_has_attr(int count, const ct_attr* attrs, const char* name) {
    if (!attrs || !name || count <= 0) return false;
    for (int i = 0; i < count; i++) {
        if (_ct_attr_name_matches(attrs[i].name, name)) return true;
    }
    return false;
}

static inline const char* ct_get_attr(int count, const ct_attr* attrs, const char* name) {
    if (!attrs || !name || count <= 0) return NULL;
    // 1. Exact match first
    for (int i = 0; i < count; i++) {
        if (attrs[i].name && _ct_safe_strcmp(attrs[i].name, name) == 0) return attrs[i].value;
    }
    // 2. Alias match ('def' <-> 'default')
    for (int i = 0; i < count; i++) {
        if (_ct_attr_name_matches(attrs[i].name, name)) return attrs[i].value;
    }
    return NULL;
}

static inline const char* ct_attr_or(int count, const ct_attr* attrs, const char* name, const char* fallback) {
    const char* val = ct_get_attr(count, attrs, name);
    return (val && val[0] != '\0') ? val : fallback;
}

static inline bool struct_has_attr(const ct_struct* s, const char* name) {
    if (name && name[0] != '\0') {
        _ct_last_queried_struct_attr = name;
    }
    bool found = s ? ct_has_attr(s->attr_count, s->attrs, name) : false;
    if (found) {
        _ct_queried_struct_attr_matches++;
    }
    return found;
}

static inline const char* struct_get_attr(const ct_struct* s, const char* name) {
    return s ? ct_get_attr(s->attr_count, s->attrs, name) : NULL;
}

static inline const char* struct_attr_or(const ct_struct* s, const char* name, const char* fallback) {
    return s ? ct_attr_or(s->attr_count, s->attrs, name, fallback) : fallback;
}

static inline bool field_has_attr(const ct_field* f, const char* name) {
    return f ? ct_has_attr(f->attr_count, f->attrs, name) : false;
}

static inline const char* field_get_attr(const ct_field* f, const char* name) {
    return f ? ct_get_attr(f->attr_count, f->attrs, name) : NULL;
}

static inline const char* field_attr_or(const ct_field* f, const char* name, const char* fallback) {
    return f ? ct_attr_or(f->attr_count, f->attrs, name, fallback) : fallback;
}

static inline bool enum_has_attr(const ct_enum* e, const char* name) {
    return e ? ct_has_attr(e->attr_count, e->attrs, name) : false;
}

static inline const char* enum_get_attr(const ct_enum* e, const char* name) {
    return e ? ct_get_attr(e->attr_count, e->attrs, name) : NULL;
}

static inline const char* enum_attr_or(const ct_enum* e, const char* name, const char* fallback) {
    return e ? ct_attr_or(e->attr_count, e->attrs, name, fallback) : fallback;
}

static inline bool variant_has_attr(const ct_variant* v, const char* name) {
    return v ? ct_has_attr(v->attr_count, v->attrs, name) : false;
}

static inline const char* variant_get_attr(const ct_variant* v, const char* name) {
    return v ? ct_get_attr(v->attr_count, v->attrs, name) : NULL;
}

static inline const char* variant_attr_or(const ct_variant* v, const char* name, const char* fallback) {
    return v ? ct_attr_or(v->attr_count, v->attrs, name, fallback) : fallback;
}

// ============================================================================
// String & Serialization Utilities (for JSON / RON / TOML Codegen)
// ============================================================================

static inline void str_to_lower(char* dest, size_t dest_size, const char* src) {
    if (!dest || dest_size == 0) return;
    size_t i = 0;
    while (src && src[i] && i + 1 < dest_size) {
        dest[i] = (char)tolower((unsigned char)src[i]);
        i++;
    }
    dest[i] = '\0';
}

static inline void str_to_upper(char* dest, size_t dest_size, const char* src) {
    if (!dest || dest_size == 0) return;
    size_t i = 0;
    while (src && src[i] && i + 1 < dest_size) {
        dest[i] = (char)toupper((unsigned char)src[i]);
        i++;
    }
    dest[i] = '\0';
}

static inline const char* str_trim_prefix(const char* str, const char* prefix) {
    if (!str || !prefix) return str ? str : "";
    size_t pre_len = _ct_safe_strlen(prefix);
    if (_ct_safe_strncmp(str, prefix, pre_len) == 0) {
        return str + pre_len;
    }
    return str;
}

static inline void str_to_snake_case(char* dest, size_t dest_size, const char* src) {
    if (!dest || dest_size == 0) return;
    size_t si = 0, di = 0;
    size_t src_len = _ct_safe_strlen(src);
    while (src && src[si] && di + 2 < dest_size) {
        char c = src[si];
        if (isupper((unsigned char)c) && si > 0) {
            char prev = src[si - 1];
            if (islower((unsigned char)prev) || (si + 1 < src_len && islower((unsigned char)src[si + 1]))) {
                dest[di++] = '_';
            }
        }
        dest[di++] = (char)tolower((unsigned char)c);
        si++;
    }
    dest[di] = '\0';
}

static inline bool str_starts_with(const char* str, const char* prefix) {
    if (!str || !prefix) return false;
    return _ct_safe_strncmp(str, prefix, _ct_safe_strlen(prefix)) == 0;
}

static inline bool str_ends_with(const char* str, const char* suffix) {
    if (!str || !suffix) return false;
    size_t l1 = _ct_safe_strlen(str);
    size_t l2 = _ct_safe_strlen(suffix);
    if (l2 > l1) return false;
    return _ct_safe_strcmp(str + l1 - l2, suffix) == 0;
}

static inline bool type_is_signed_int(TypeKind k) {
    return k == TYPE_INT8 || k == TYPE_INT16 || k == TYPE_INT32 || k == TYPE_INT64;
}

static inline bool type_is_unsigned_int(TypeKind k) {
    return k == TYPE_UINT8 || k == TYPE_UINT16 || k == TYPE_UINT32 || k == TYPE_UINT64;
}

static inline bool type_is_integer(TypeKind k) {
    return type_is_signed_int(k) || type_is_unsigned_int(k);
}

static inline bool type_is_float(TypeKind k) {
    return k == TYPE_FLOAT || k == TYPE_DOUBLE;
}

static inline bool type_is_numeric(TypeKind k) {
    return type_is_integer(k) || type_is_float(k);
}

static inline bool type_is_string(TypeKind k) {
    return k == TYPE_STRING;
}

static inline const char* type_printf_spec(TypeKind k) {
    switch (k) {
        case TYPE_INT8:
        case TYPE_INT16:
        case TYPE_INT32:  return "%d";
        case TYPE_INT64:  return "%lld";
        case TYPE_UINT8:
        case TYPE_UINT16:
        case TYPE_UINT32: return "%u";
        case TYPE_UINT64: return "%llu";
        case TYPE_FLOAT:
        case TYPE_DOUBLE: return "%g";
        case TYPE_BOOL:   return "%s";
        case TYPE_CHAR:   return "%c";
        case TYPE_STRING: return "%s";
        default:          return "%p";
    }
}

static inline const ct_struct* field_struct_type(const ct_field* f) {
    if (!f || !f->type) return NULL;
    const char* t = str_trim_prefix(f->type, "struct ");
    return find_struct(t);
}

static inline const ct_enum* field_enum_type(const ct_field* f) {
    if (!f || !f->type) return NULL;
    const char* t = str_trim_prefix(f->type, "enum ");
    return find_enum(t);
}

// ============================================================================
// Dedicated In-Memory Code Emission Buffer (Thread-Safe, Zero-Stdout-Hijack)
// ============================================================================

static char* _ct_emit_buf = NULL;
static size_t _ct_emit_len = 0;
static size_t _ct_emit_cap = 0;
static int _ct_emit_used = 0;

static inline void _ct_append_bytes(const char* data, size_t len) {
    if (!data || len == 0) return;
    if (_ct_emit_len + len + 1 > _ct_emit_cap) {
        size_t new_cap = _ct_emit_cap == 0 ? 8192 : _ct_emit_cap * 2;
        while (new_cap < _ct_emit_len + len + 1) {
            new_cap *= 2;
        }
        char* grown = (char*)realloc(_ct_emit_buf, new_cap);
        if (!grown) return;
        _ct_emit_buf = grown;
        _ct_emit_cap = new_cap;
    }
    memcpy(_ct_emit_buf + _ct_emit_len, data, len);
    _ct_emit_len += len;
    _ct_emit_buf[_ct_emit_len] = '\0';
}

static inline void emit(const char* str) {
    _ct_emit_used = 1;
    if (str) {
        _ct_append_bytes(str, _ct_safe_strlen(str));
    }
}

static inline void emitf(const char* fmt, ...) {
    _ct_emit_used = 1;
    if (!fmt) return;
    char stack_buf[4096];
    va_list args;
    va_start(args, fmt);
    int n = vsnprintf(stack_buf, sizeof(stack_buf), fmt, args);
    va_end(args);
    if (n <= 0) return;
    if ((size_t)n < sizeof(stack_buf)) {
        _ct_append_bytes(stack_buf, (size_t)n);
    } else {
        char* heap_buf = (char*)malloc((size_t)n + 1);
        if (heap_buf) {
            va_start(args, fmt);
            vsnprintf(heap_buf, (size_t)n + 1, fmt, args);
            va_end(args);
            _ct_append_bytes(heap_buf, (size_t)n);
            free(heap_buf);
        }
    }
}

static inline void emit_file(const char* path, const char* content) {
    if (!path || !content) return;
    FILE* f = fopen(path, "w");
    if (f) {
        fputs(content, f);
        fclose(f);
    } else {
        fprintf(stderr, "Comptime error: cannot write to '%s'\n", path);
    }
}

const char* __crupp_get_emitted_code(void) {
    return _ct_emit_buf ? _ct_emit_buf : "";
}

int __crupp_was_emit_used(void) {
    return _ct_emit_used;
}

const char* __crupp_get_unmatched_struct_attr(void) {
    if (_ct_last_queried_struct_attr && _ct_queried_struct_attr_matches == 0) {
        return _ct_last_queried_struct_attr;
    }
    return NULL;
}

int __crupp_get_struct_count(void) {
    return _CT_STRUCT_COUNT;
}

void __crupp_free_emitted_code(void) {
    if (_ct_emit_buf) {
        free(_ct_emit_buf);
        _ct_emit_buf = NULL;
        _ct_emit_len = 0;
        _ct_emit_cap = 0;
    }
    _ct_emit_used = 0;
    _ct_last_queried_struct_attr = NULL;
    _ct_queried_struct_attr_matches = 0;
}

// ============================================================================
// Ergonomic Iteration Macros (Optional Syntactic Sugar)
// ============================================================================

#define for_each_struct(var) \
    for (int _i = 0; _i < struct_count(); _i++) \
        for (const ct_struct* var = get_struct(_i); var; var = NULL)

#define for_each_field(st, var) \
    for (int _f = 0; (st) && _f < (st)->field_count; _f++) \
        for (const ct_field* var = &(st)->fields[_f]; var; var = NULL)

#define for_each_enum(var) \
    for (int _i = 0; _i < enum_count(); _i++) \
        for (const ct_enum* var = get_enum(_i); var; var = NULL)

#define for_each_variant(en, var) \
    for (int _v = 0; (en) && _v < (en)->variant_count; _v++) \
        for (const ct_variant* var = &(en)->variants[_v]; var; var = NULL)

// ============================================================================
// Clangd / LSP Seamless Integration (Active only in IDE / clangd, never in TCC)
// ============================================================================
#ifndef __TINYC__
#if defined(__CRUPP_LSP_GEN_HEADER__)
#if defined(__has_include)
#if __has_include(__CRUPP_LSP_GEN_HEADER__)
#include __CRUPP_LSP_GEN_HEADER__
#endif
#endif
#endif
#define _CRUPP_LSP_CAT2(a, b) a##b
#define _CRUPP_LSP_CAT(a, b) _CRUPP_LSP_CAT2(a, b)
#define main _CRUPP_LSP_CAT(__crupp_comptime_main_, __COUNTER__)
#endif

#endif /* COMPTIME_H */
"#,
    );

    s
}
