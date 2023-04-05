use std::{cmp::Ordering, fmt};

use crate::{
    algorithm::pervade::*, array::*, function::Function, grid_fmt::GridFmt, Uiua, UiuaResult,
};

#[derive(Clone)]
pub enum Value {
    Num(Array<f64>),
    Byte(Array<u8>),
    Char(Array<char>),
    Func(Array<Function>),
}

impl Default for Value {
    fn default() -> Self {
        Array::<u8>::default().into()
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(array) => array.fmt(f),
            Self::Byte(array) => array.fmt(f),
            Self::Char(array) => array.fmt(f),
            Self::Func(array) => array.fmt(f),
        }
    }
}

impl Value {
    pub fn from_row_values(
        values: impl IntoIterator<Item = Value>,
        env: &Uiua,
    ) -> UiuaResult<Self> {
        let mut row_values = values.into_iter();
        let Some(mut value) = row_values.next() else {
            return Ok(Value::default());
        };
        for row in row_values {
            value = value.join(row, env)?;
        }
        Ok(value)
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Num(_) => "number",
            Self::Byte(_) => "byte",
            Self::Char(_) => "char",
            Self::Func(_) => "function",
        }
    }
    pub fn shape(&self) -> &[usize] {
        match self {
            Self::Num(array) => array.shape(),
            Self::Byte(array) => array.shape(),
            Self::Char(array) => array.shape(),
            Self::Func(array) => array.shape(),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            Self::Num(array) => array.row_count(),
            Self::Byte(array) => array.row_count(),
            Self::Char(array) => array.row_count(),
            Self::Func(array) => array.row_count(),
        }
    }
    pub fn rank(&self) -> usize {
        self.shape().len()
    }
    pub fn generic_mut<T>(
        &mut self,
        n: impl FnOnce(&mut Array<f64>) -> T,
        b: impl FnOnce(&mut Array<u8>) -> T,
        c: impl FnOnce(&mut Array<char>) -> T,
        f: impl FnOnce(&mut Array<Function>) -> T,
    ) -> T {
        match self {
            Self::Num(array) => n(array),
            Self::Byte(array) => b(array),
            Self::Char(array) => c(array),
            Self::Func(array) => f(array),
        }
    }
    pub fn into_generic<T>(
        self,
        n: impl FnOnce(Array<f64>) -> T,
        b: impl FnOnce(Array<u8>) -> T,
        c: impl FnOnce(Array<char>) -> T,
        f: impl FnOnce(Array<Function>) -> T,
    ) -> T {
        match self {
            Self::Num(array) => n(array),
            Self::Byte(array) => b(array),
            Self::Char(array) => c(array),
            Self::Func(array) => f(array),
        }
    }
    pub fn show(&self) -> String {
        match self {
            Self::Num(array) => array.grid_string(),
            Self::Byte(array) => array.grid_string(),
            Self::Char(array) => array.grid_string(),
            Self::Func(array) => array.grid_string(),
        }
    }
    pub fn as_indices(&self, env: &Uiua, requirement: &'static str) -> UiuaResult<Vec<isize>> {
        self.as_number_list(env, requirement, |f| f % 1.0 == 0.0, |f| f as isize)
    }
    pub fn as_nat(&self, env: &Uiua, requirement: &'static str) -> UiuaResult<usize> {
        Ok(match self {
            Value::Num(nums) => {
                if nums.rank() > 0 {
                    return Err(
                        env.error(format!("{requirement}, but its rank is {}", nums.rank()))
                    );
                }
                let num = nums.data[0];
                if num < 0.0 {
                    return Err(env.error(format!("{requirement}, but it is negative")));
                }
                if num.fract().abs() > f64::EPSILON {
                    return Err(env.error(format!("{requirement}, but it has a fractional part")));
                }
                num as usize
            }
            Value::Byte(bytes) => {
                if bytes.rank() > 0 {
                    return Err(
                        env.error(format!("{requirement}, but its rank is {}", bytes.rank()))
                    );
                }
                bytes.data[0] as usize
            }
            value => {
                return Err(env.error(format!("{requirement}, but it is {}", value.type_name())))
            }
        })
    }
    pub fn as_num(&self, env: &Uiua, requirement: &'static str) -> UiuaResult<f64> {
        Ok(match self {
            Value::Num(nums) => {
                if nums.rank() > 0 {
                    return Err(
                        env.error(format!("{requirement}, but its rank is {}", nums.rank()))
                    );
                }
                nums.data[0]
            }
            Value::Byte(bytes) => {
                if bytes.rank() > 0 {
                    return Err(
                        env.error(format!("{requirement}, but its rank is {}", bytes.rank()))
                    );
                }
                bytes.data[0] as f64
            }
            value => {
                return Err(env.error(format!("{requirement}, but it is {}", value.type_name())))
            }
        })
    }
    pub fn as_naturals(&self, env: &Uiua, requirement: &'static str) -> UiuaResult<Vec<usize>> {
        self.as_number_list(
            env,
            requirement,
            |f| f % 1.0 == 0.0 && f >= 0.0,
            |f| f as usize,
        )
    }
    fn as_number_list<T>(
        &self,
        env: &Uiua,
        requirement: &'static str,
        test: fn(f64) -> bool,
        convert: fn(f64) -> T,
    ) -> UiuaResult<Vec<T>> {
        Ok(match self {
            Value::Num(nums) => {
                if nums.rank() > 1 {
                    return Err(
                        env.error(format!("{requirement}, but its rank is {}", nums.rank()))
                    );
                }
                let mut result = Vec::with_capacity(nums.row_count());
                for &num in nums.data() {
                    if !test(num) {
                        return Err(env.error(requirement));
                    }
                    result.push(convert(num));
                }
                result
            }
            Value::Byte(bytes) => {
                if bytes.rank() > 1 {
                    return Err(
                        env.error(format!("{requirement}, but its rank is {}", bytes.rank()))
                    );
                }
                let mut result = Vec::with_capacity(bytes.row_count());
                for &byte in bytes.data() {
                    let num = byte as f64;
                    if !test(num) {
                        return Err(env.error(requirement));
                    }
                    result.push(convert(num));
                }
                result
            }
            value => {
                return Err(env.error(format!(
                    "{requirement}, but its type is {}",
                    value.type_name()
                )))
            }
        })
    }
    pub fn as_string(&self, env: &Uiua, requirement: &'static str) -> UiuaResult<String> {
        if let Value::Char(chars) = self {
            if chars.rank() > 1 {
                return Err(env.error(format!("{requirement}, but its rank is {}", chars.rank())));
            }
            Ok(chars.data().iter().collect())
        } else {
            Err(env.error(format!(
                "{requirement}, but its type is {}",
                self.type_name()
            )))
        }
    }
    pub fn into_bytes(self, env: &Uiua, requirement: &'static str) -> UiuaResult<Vec<u8>> {
        Ok(match self {
            Value::Byte(a) => {
                if a.rank() != 1 {
                    return Err(env.error(format!("{requirement}, but its rank is {}", a.rank())));
                }
                a.data
            }
            Value::Num(a) => {
                if a.rank() != 1 {
                    return Err(env.error(format!("{requirement}, but its rank is {}", a.rank())));
                }
                a.data.iter().map(|&f| f as u8).collect()
            }
            Value::Char(a) => {
                if a.rank() != 1 {
                    return Err(env.error(format!("{requirement}, but its rank is {}", a.rank())));
                }
                a.data.into_iter().collect::<String>().into_bytes()
            }
            value => {
                return Err(env.error(format!(
                    "{requirement}, but its type is {}",
                    value.type_name()
                )))
            }
        })
    }
}

macro_rules! value_from {
    ($ty:ty, $variant:ident) => {
        impl From<$ty> for Value {
            fn from(item: $ty) -> Self {
                Self::$variant(Array::from(item))
            }
        }
        impl From<Array<$ty>> for Value {
            fn from(array: Array<$ty>) -> Self {
                Self::$variant(array)
            }
        }
        impl From<Vec<$ty>> for Value {
            fn from(vec: Vec<$ty>) -> Self {
                Self::$variant(Array::from(vec))
            }
        }
        impl From<(Vec<usize>, Vec<$ty>)> for Value {
            fn from((shape, data): (Vec<usize>, Vec<$ty>)) -> Self {
                Self::$variant(Array::new(shape, data))
            }
        }
        impl FromIterator<$ty> for Value {
            fn from_iter<I: IntoIterator<Item = $ty>>(iter: I) -> Self {
                Self::$variant(Array::from_iter(iter))
            }
        }
    };
}

value_from!(f64, Num);
value_from!(u8, Byte);
value_from!(char, Char);
value_from!(Function, Func);

impl FromIterator<usize> for Value {
    fn from_iter<I: IntoIterator<Item = usize>>(iter: I) -> Self {
        iter.into_iter().map(|i| i as f64).collect()
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::from(b as u8)
    }
}

impl From<usize> for Value {
    fn from(i: usize) -> Self {
        Value::from(i as f64)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        s.chars().collect()
    }
}

macro_rules! value_un_impl {
    ($name:ident, $(($variant:ident, $f:ident)),* $(,)?) => {
        impl Value {
            pub fn $name(mut self, env: &Uiua) -> UiuaResult<Self> {
                Ok(match self {
                    $(Self::$variant(array) => {
                        let (shape, data) = array.into_pair();
                        (shape, data.into_iter().map($name::$f).collect::<Vec<_>>()).into()
                    },)*
                    val => return Err($name::error(val.type_name(), env))
                })
            }
        }
    }
}

macro_rules! value_un_impl_all {
    ($($name:ident),* $(,)?) => {
        $(value_un_impl!($name, (Num, num), (Byte, byte));)*
    }
}

value_un_impl_all!(neg, not, abs, sign, sqrt, sin, cos, asin, acos, floor, ceil, round);

macro_rules! value_bin_impl {
    ($name:ident, $(($va:ident, $vb:ident, $f:ident)),* $(,)?) => {
        impl Value {
            pub fn $name(&self, other: &Self, env: &Uiua) -> UiuaResult<Self> {
                Ok(match (self, other) {
                    $((Value::$va(a), Value::$vb(b)) => {
                        bin_pervade(a, b, env, $name::$f)?.into()
                    },)*
                    (a, b) => return Err($name::error(a.type_name(), b.type_name(), env)),
                })
            }
        }
    };
}

value_bin_impl!(
    add,
    (Num, Num, num_num),
    (Num, Char, num_char),
    (Char, Num, char_num),
    (Byte, Byte, byte_byte),
    (Byte, Char, byte_char),
    (Char, Byte, char_byte),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);

value_bin_impl!(
    sub,
    (Num, Num, num_num),
    (Num, Char, num_char),
    (Char, Char, char_char),
    (Byte, Byte, byte_byte),
    (Byte, Char, byte_char),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);

value_bin_impl!(
    mul,
    (Num, Num, num_num),
    (Byte, Byte, byte_byte),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);
value_bin_impl!(
    div,
    (Num, Num, num_num),
    (Byte, Byte, byte_byte),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);
value_bin_impl!(
    modulus,
    (Num, Num, num_num),
    (Byte, Byte, byte_byte),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);
value_bin_impl!(
    pow,
    (Num, Num, num_num),
    (Byte, Byte, byte_byte),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);
value_bin_impl!(
    log,
    (Num, Num, num_num),
    (Byte, Byte, byte_byte),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);
value_bin_impl!(atan2, (Num, Num, num_num));

value_bin_impl!(
    min,
    (Num, Num, num_num),
    (Char, Char, char_char),
    (Char, Num, char_num),
    (Num, Char, num_char),
    (Byte, Byte, byte_byte),
    (Char, Byte, char_byte),
    (Byte, Char, byte_char),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);

value_bin_impl!(
    max,
    (Num, Num, num_num),
    (Char, Char, char_char),
    (Char, Num, char_num),
    (Num, Char, num_char),
    (Byte, Byte, byte_byte),
    (Char, Byte, char_byte),
    (Byte, Char, byte_char),
    (Byte, Num, byte_num),
    (Num, Byte, num_byte),
);

macro_rules! cmp_impls {
    ($($name:ident),*) => {
        $(
            value_bin_impl!(
                $name,
                // Value comparable
                (Num, Num, num_num),
                (Byte, Byte, generic),
                (Char, Char, generic),
                (Num, Byte, num_byte),
                (Byte, Num, byte_num),
                // Type comparable
                (Num, Char, always_less),
                (Num, Func, always_less),
                (Byte, Char, always_less),
                (Byte, Func, always_less),
                (Char, Num, always_greater),
                (Char, Byte, always_greater),
                (Char, Func, always_less),
            );
        )*
    };
}

cmp_impls!(is_eq, is_ne, is_lt, is_le, is_gt, is_ge);

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Num(a), Value::Num(b)) => a == b,
            (Value::Byte(a), Value::Byte(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Func(a), Value::Func(b)) => a == b,
            (Value::Num(a), Value::Byte(b)) => a.val_eq(b),
            (Value::Byte(a), Value::Num(b)) => b.val_eq(a),
            _ => false,
        }
    }
}

impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Value::Num(a), Value::Num(b)) => a.val_cmp(b),
            (Value::Byte(a), Value::Byte(b)) => a.val_cmp(b),
            (Value::Char(a), Value::Char(b)) => a.val_cmp(b),
            (Value::Func(a), Value::Func(b)) => a.val_cmp(b),
            (Value::Num(a), Value::Byte(b)) => a.val_cmp(b),
            (Value::Byte(a), Value::Num(b)) => b.val_cmp(a).reverse(),
            (Value::Num(_), _) => Ordering::Less,
            (_, Value::Num(_)) => Ordering::Greater,
            (Value::Byte(_), _) => Ordering::Less,
            (_, Value::Byte(_)) => Ordering::Greater,
            (Value::Char(_), _) => Ordering::Less,
            (_, Value::Char(_)) => Ordering::Greater,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(n) => n.fmt(f),
            Value::Byte(b) => b.fmt(f),
            Value::Char(c) => c.fmt(f),
            Value::Func(func) => func.fmt(f),
        }
    }
}
