//! Algorithms for zipping modifiers

use std::{cell::RefCell, collections::HashMap, iter::repeat, mem::swap, rc::Rc, slice};

use ecow::eco_vec;

use crate::{
    algorithm::pervade::bin_pervade_generic, cowslice::CowSlice, function::Function, random,
    value::Value, Array, ArrayValue, Boxed, Complex, ImplPrimitive, Instr, PersistentMeta,
    Primitive, Shape, Uiua, UiuaResult,
};

use super::{fill_value_shapes, fixed_rows, multi_output, FillContext, FixedRowsData, MultiOutput};

type ValueMonFn = Rc<dyn Fn(Value, usize, &mut Uiua) -> UiuaResult<Value>>;
type ValueMon2Fn = Box<dyn Fn(Value, usize, &mut Uiua) -> UiuaResult<(Value, Value)>>;
type ValueDyFn = Box<dyn Fn(Value, Value, usize, usize, &mut Uiua) -> UiuaResult<Value>>;

fn mon_fn(f: impl Fn(Value, usize, &mut Uiua) -> UiuaResult<Value> + 'static) -> ValueMonFn {
    Rc::new(move |v, d, env| f(v, d, env))
}

fn spanned_mon_fn(
    span: usize,
    f: impl Fn(Value, usize, &Uiua) -> UiuaResult<Value> + 'static,
) -> ValueMonFn {
    mon_fn(move |v, d, env| env.with_span(span, |env| f(v, d, env)))
}

fn spanned_mon2_fn(
    span: usize,
    f: impl Fn(Value, usize, &Uiua) -> UiuaResult<(Value, Value)> + 'static,
) -> ValueMon2Fn {
    Box::new(move |v, d, env| env.with_span(span, |env| f(v, d, env)))
}

fn prim_mon_fast_fn(prim: Primitive, span: usize) -> Option<ValueMonFn> {
    use Primitive::*;
    Some(match prim {
        Not => spanned_mon_fn(span, |v, _, env| Value::not(v, env)),
        Sign => spanned_mon_fn(span, |v, _, env| Value::sign(v, env)),
        Neg => spanned_mon_fn(span, |v, _, env| Value::neg(v, env)),
        Abs => spanned_mon_fn(span, |v, _, env| Value::abs(v, env)),
        Sqrt => spanned_mon_fn(span, |v, _, env| Value::sqrt(v, env)),
        Floor => spanned_mon_fn(span, |v, _, env| Value::floor(v, env)),
        Ceil => spanned_mon_fn(span, |v, _, env| Value::ceil(v, env)),
        Round => spanned_mon_fn(span, |v, _, env| Value::round(v, env)),
        Deshape => spanned_mon_fn(span, |mut v, d, _| {
            Value::deshape_depth(&mut v, d);
            Ok(v)
        }),
        Transpose => spanned_mon_fn(span, |mut v, d, _| {
            Value::transpose_depth(&mut v, d, 1);
            Ok(v)
        }),
        Reverse => spanned_mon_fn(span, |mut v, d, _| {
            Value::reverse_depth(&mut v, d);
            Ok(v)
        }),
        Classify => spanned_mon_fn(span, |v, d, _| Ok(v.classify_depth(d))),
        Fix => spanned_mon_fn(span, |mut v, d, _| {
            v.fix_depth(d);
            Ok(v)
        }),
        Box => spanned_mon_fn(span, |v, d, _| Ok(v.box_depth(d))),
        _ => return None,
    })
}

fn impl_prim_mon_fast_fn(prim: ImplPrimitive, span: usize) -> Option<ValueMonFn> {
    use ImplPrimitive::*;
    Some(match prim {
        TransposeN(n) => spanned_mon_fn(span, move |mut v, d, _| {
            Value::transpose_depth(&mut v, d, n);
            Ok(v)
        }),
        ReplaceRand => spanned_mon_fn(span, |v, d, _| {
            let shape = &v.shape()[..d.min(v.rank())];
            let elem_count: usize = shape.iter().product();
            let mut data = eco_vec![0.0; elem_count];
            for n in data.make_mut() {
                *n = random();
            }
            Ok(Array::new(shape, data).into())
        }),
        SortUp => spanned_mon_fn(span, |mut v, d, _| {
            v.sort_up_depth(d);
            Ok(v)
        }),
        SortDown => spanned_mon_fn(span, |mut v, d, _| {
            v.sort_down_depth(d);
            Ok(v)
        }),
        _ => return None,
    })
}

fn prim_mon2_fast_fn(prim: Primitive, span: usize) -> Option<ValueMon2Fn> {
    use Primitive::*;
    Some(match prim {
        Dup => spanned_mon2_fn(span, |v, _, _| Ok((v.clone(), v))),
        _ => return None,
    })
}

fn impl_prim_mon2_fast_fn(prim: ImplPrimitive, span: usize) -> Option<ValueMon2Fn> {
    use ImplPrimitive::*;
    Some(match prim {
        UnCouple => spanned_mon2_fn(span, |v, d, env| v.uncouple_depth(d, env)),
        UnJoin => spanned_mon2_fn(span, |v, d, env| v.unjoin_depth(d, env)),
        _ => return None,
    })
}

fn f_mon_fast_fn(f: &Function, env: &Uiua) -> Option<(ValueMonFn, usize)> {
    thread_local! {
        static CACHE: RefCell<HashMap<u64, Option<(ValueMonFn, usize)>>>
            = RefCell::new(HashMap::new());
    }
    CACHE.with(|cache| {
        if !cache.borrow().contains_key(&f.hash()) {
            let f_and_depth = f_mon_fast_fn_impl(f.instrs(&env.asm), false, env);
            cache.borrow_mut().insert(f.hash(), f_and_depth);
        }
        cache.borrow()[&f.hash()].clone()
    })
}

fn f_mon_fast_fn_impl(instrs: &[Instr], deep: bool, env: &Uiua) -> Option<(ValueMonFn, usize)> {
    use Primitive::*;
    Some(match instrs {
        &[Instr::Prim(prim, span)] => {
            let f = prim_mon_fast_fn(prim, span)?;
            (f, 0)
        }
        &[Instr::ImplPrim(prim, span)] => {
            let f = impl_prim_mon_fast_fn(prim, span)?;
            (f, 0)
        }
        [Instr::PushFunc(f), Instr::Prim(Rows, _)] => {
            let (f, d) = f_mon_fast_fn(f, env)?;
            (f, d + 1)
        }
        [Instr::Prim(Pop, _), Instr::Push(repl)] => {
            let replacement = repl.clone();
            (
                Rc::new(move |val, depth, _| Ok(val.replace_depth(replacement.clone(), depth))),
                0,
            )
        }
        instrs if !deep => {
            let mut start = 0;
            let mut depth = 0;
            let mut func: Option<ValueMonFn> = None;
            'outer: loop {
                for len in [1, 2] {
                    let end = start + len;
                    if end > instrs.len() {
                        break 'outer;
                    }
                    let instrs = &instrs[start..end];
                    let Some((f, d)) = f_mon_fast_fn_impl(instrs, true, env) else {
                        continue;
                    };
                    depth += d;
                    func = func.map(|func| {
                        mon_fn(
                            move |val: Value, depth: usize, env: &mut Uiua| -> UiuaResult<Value> {
                                f(func(val, depth, env)?, depth, env)
                            },
                        )
                    });
                    start = end;
                    continue 'outer;
                }
                return None;
            }
            let func = func?;
            (func, depth)
        }
        _ => return None,
    })
}

fn f_mon2_fast_fn(f: &Function, env: &Uiua) -> Option<(ValueMon2Fn, usize)> {
    use Primitive::*;
    Some(match f.instrs(&env.asm) {
        &[Instr::Prim(prim, span)] => {
            let f = prim_mon2_fast_fn(prim, span)?;
            (f, 0)
        }
        &[Instr::ImplPrim(prim, span)] => {
            let f = impl_prim_mon2_fast_fn(prim, span)?;
            (f, 0)
        }
        [Instr::PushFunc(f), Instr::Prim(Rows, _)] => {
            let (f, d) = f_mon2_fast_fn(f, env)?;
            (f, d + 1)
        }
        _ => return None,
    })
}

impl Value {
    fn replace_depth(&self, mut replacement: Value, depth: usize) -> Value {
        let depth = self.rank().min(depth);
        let prefix = &self.shape()[..depth];
        replacement.generic_mut_shallow(
            |arr| arr.repeat_shape(Shape::from(prefix)),
            |arr| arr.repeat_shape(Shape::from(prefix)),
            |arr| arr.repeat_shape(Shape::from(prefix)),
            |arr| arr.repeat_shape(Shape::from(prefix)),
            |arr| arr.repeat_shape(Shape::from(prefix)),
        );
        replacement
    }
}

impl<T: Clone> Array<T> {
    fn repeat_shape(&mut self, mut shape_prefix: Shape) {
        let count = shape_prefix.elements();
        swap(&mut self.shape, &mut shape_prefix);
        self.shape.extend(shape_prefix);
        match count {
            0 => {
                self.data = CowSlice::default();
            }
            1 => {}
            n => {
                let mut new_data = self.data.clone();
                for _ in 1..n {
                    new_data.extend_from_slice(&self.data);
                }
                self.data = new_data;
            }
        }
        self.validate_shape();
    }
}

fn spanned_dy_fn(
    span: usize,
    f: impl Fn(Value, Value, usize, usize, &Uiua) -> UiuaResult<Value> + 'static,
) -> ValueDyFn {
    Box::new(move |a, b, ad, bd, env| env.with_span(span, |env| f(a, b, ad, bd, env)))
}

fn prim_dy_fast_fn(prim: Primitive, span: usize) -> Option<ValueDyFn> {
    use std::boxed::Box;
    use Primitive::*;
    Some(match prim {
        Add => spanned_dy_fn(span, Value::add),
        Sub => spanned_dy_fn(span, Value::sub),
        Mul => spanned_dy_fn(span, Value::mul),
        Div => spanned_dy_fn(span, Value::div),
        Pow => spanned_dy_fn(span, Value::pow),
        Mod => spanned_dy_fn(span, Value::modulus),
        Log => spanned_dy_fn(span, Value::log),
        Eq => spanned_dy_fn(span, Value::is_eq),
        Ne => spanned_dy_fn(span, Value::is_ne),
        Lt => spanned_dy_fn(span, Value::is_lt),
        Gt => spanned_dy_fn(span, Value::is_gt),
        Le => spanned_dy_fn(span, Value::is_le),
        Ge => spanned_dy_fn(span, Value::is_ge),
        Complex => spanned_dy_fn(span, Value::complex),
        Max => spanned_dy_fn(span, Value::max),
        Min => spanned_dy_fn(span, Value::min),
        Atan => spanned_dy_fn(span, Value::atan2),
        Rotate => Box::new(move |a, b, ad, bd, env| {
            env.with_span(span, |env| a.rotate_depth(b, ad, bd, env))
        }),
        _ => return None,
    })
}

pub(crate) fn f_dy_fast_fn(instrs: &[Instr], env: &Uiua) -> Option<(ValueDyFn, usize, usize)> {
    use std::boxed::Box;
    use Primitive::*;

    fn nest_dy_fast<F>(
        (f, ad1, bd1): (F, usize, usize),
        ad2: usize,
        bd2: usize,
    ) -> Option<(F, usize, usize)> {
        if (ad1 as isize - ad2 as isize).signum() != (bd1 as isize - bd2 as isize).signum() {
            return None;
        }
        Some((f, ad1 + ad2, bd1 + bd2))
    }

    match instrs {
        &[Instr::Prim(prim, span)] => {
            let f = prim_dy_fast_fn(prim, span)?;
            return Some((f, 0, 0));
        }
        [Instr::PushFunc(f), Instr::Prim(Rows, _)] => {
            return nest_dy_fast(f_dy_fast_fn(f.instrs(&env.asm), env)?, 1, 1)
        }
        [Instr::Prim(Flip, _), rest @ ..] => {
            let (f, a, b) = f_dy_fast_fn(rest, env)?;
            let f = Box::new(move |a, b, ad, bd, env: &mut Uiua| f(b, a, bd, ad, env));
            return Some((f, a, b));
        }
        _ => (),
    }
    None
}

pub fn each(env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    let f = env.pop_function()?;
    let sig = f.signature();
    match sig.args {
        0 => env.without_fill(|env| env.call(f)),
        1 => each1(f, env.pop(1)?, env),
        2 => each2(f, env.pop(1)?, env.pop(2)?, env),
        n => {
            let mut args = Vec::with_capacity(n);
            for i in 0..n {
                args.push(env.pop(i + 1)?);
            }
            eachn(f, args, env)
        }
    }
}

fn each1(f: Function, mut xs: Value, env: &mut Uiua) -> UiuaResult {
    if let Some((f, ..)) = f_mon_fast_fn(&f, env) {
        let maybe_through_boxes = matches!(&xs, Value::Box(..));
        if !maybe_through_boxes {
            let rank = xs.rank();
            let val = f(xs, rank, env)?;
            env.push(val);
            return Ok(());
        }
    }
    let outputs = f.signature().outputs;
    let mut new_values = multi_output(outputs, Vec::with_capacity(xs.element_count()));
    let new_shape = xs.shape().clone();
    let is_empty = outputs > 0 && xs.row_count() == 0;
    let per_meta = xs.take_per_meta();
    env.without_fill(|env| -> UiuaResult {
        if is_empty {
            env.push(xs.proxy_scalar(env));
            _ = env.call_maintain_sig(f);
            for i in 0..outputs {
                new_values[i].push(env.pop("each's function result")?);
            }
        } else {
            for val in xs.into_elements() {
                env.push(val);
                env.call(f.clone())?;
                for i in 0..outputs {
                    new_values[i].push(env.pop("each's function result")?);
                }
            }
        }
        Ok(())
    })?;
    for new_values in new_values.into_iter().rev() {
        let mut new_shape = new_shape.clone();
        let mut eached = Value::from_row_values(new_values, env)?;
        if is_empty {
            eached.pop_row();
        }
        new_shape.extend_from_slice(eached.shape().row_slice());
        *eached.shape_mut() = new_shape;
        eached.validate_shape();
        eached.set_per_meta(per_meta.clone());
        env.push(eached);
    }
    Ok(())
}

fn each2(f: Function, mut xs: Value, mut ys: Value, env: &mut Uiua) -> UiuaResult {
    fill_value_shapes(&mut xs, &mut ys, true, env)?;
    if let Some((f, ..)) = f_dy_fast_fn(f.instrs(&env.asm), env) {
        let xrank = xs.rank();
        let yrank = ys.rank();
        let val = f(xs, ys, xrank, yrank, env)?;
        env.push(val);
    } else {
        let outputs = f.signature().outputs;
        let mut xs_shape = xs.shape().to_vec();
        let mut ys_shape = ys.shape().to_vec();
        let is_empty = outputs > 0 && (xs.row_count() == 0 || ys.row_count() == 0);
        let per_meta = xs.take_per_meta().xor(ys.take_per_meta());
        let (new_shape, new_values) = env.without_fill(|env| {
            if is_empty {
                if let Some(r) = xs_shape.first_mut() {
                    *r = 1;
                }
                if let Some(r) = ys_shape.first_mut() {
                    *r = 1;
                }
                bin_pervade_generic(
                    &xs_shape,
                    slice::from_ref(&xs.proxy_scalar(env)),
                    &ys_shape,
                    slice::from_ref(&ys.proxy_scalar(env)),
                    env,
                    |x, y, env| {
                        env.push(y);
                        env.push(x);
                        if env.call_maintain_sig(f.clone()).is_ok() {
                            (0..outputs)
                                .map(|_| env.pop("each's function result"))
                                .collect::<Result<MultiOutput<_>, _>>()
                        } else {
                            Ok(multi_output(outputs, Value::default()))
                        }
                    },
                )
            } else {
                let xs_values: Vec<_> = xs.into_elements().collect();
                let ys_values: Vec<_> = ys.into_elements().collect();
                bin_pervade_generic(
                    &xs_shape,
                    xs_values,
                    &ys_shape,
                    ys_values,
                    env,
                    |x, y, env| {
                        env.push(y);
                        env.push(x);
                        env.call(f.clone())?;
                        (0..outputs)
                            .map(|_| env.pop("each's function result"))
                            .collect::<Result<MultiOutput<_>, _>>()
                    },
                )
            }
        })?;
        let mut transposed = multi_output(outputs, Vec::with_capacity(new_values.len()));
        for values in new_values {
            for (i, value) in values.into_iter().enumerate() {
                transposed[i].push(value);
            }
        }
        for new_values in transposed {
            let mut new_shape = new_shape.clone();
            let mut eached = Value::from_row_values(new_values, env)?;
            if is_empty {
                eached.pop_row();
                if let Some(r) = new_shape.first_mut() {
                    *r -= 1;
                }
            }
            new_shape.extend_from_slice(&eached.shape().row());
            *eached.shape_mut() = new_shape;
            eached.set_per_meta(per_meta.clone());
            env.push(eached);
        }
    }
    Ok(())
}

fn eachn(f: Function, mut args: Vec<Value>, env: &mut Uiua) -> UiuaResult {
    for a in 0..args.len() - 1 {
        let (a, b) = args.split_at_mut(a + 1);
        let a = a.last_mut().unwrap();
        for b in b {
            fill_value_shapes(a, b, true, env)?;
        }
    }
    let outputs = f.signature().outputs;
    let is_empty = outputs > 0 && args.iter().any(|v| v.row_count() == 0);
    let elem_count = args.iter().map(Value::element_count).max().unwrap() + is_empty as usize;
    let mut new_values = multi_output(outputs, Vec::with_capacity(elem_count));
    let new_shape = args
        .iter()
        .map(Value::shape)
        .max_by_key(|s| s.len())
        .unwrap()
        .clone();
    let per_meta = PersistentMeta::xor_all(args.iter_mut().map(|v| v.take_per_meta()));
    env.without_fill(|env| -> UiuaResult {
        if is_empty {
            for arg in args.into_iter().rev() {
                env.push(arg.proxy_scalar(env));
            }
            _ = env.call_maintain_sig(f);
            for i in 0..outputs {
                new_values[i].push(env.pop("each's function result")?);
            }
        } else {
            let mut arg_elems: Vec<_> = args
                .into_iter()
                .map(|val| {
                    let repetitions = elem_count / val.element_count();
                    val.into_elements()
                        .flat_map(move |elem| repeat(elem).take(repetitions))
                })
                .collect();
            for _ in 0..elem_count {
                for arg in arg_elems.iter_mut().rev() {
                    env.push(arg.next().unwrap());
                }
                env.call(f.clone())?;
                for i in 0..outputs {
                    new_values[i].push(env.pop("each's function result")?);
                }
            }
        }
        Ok(())
    })?;
    for new_values in new_values.into_iter().rev() {
        let mut new_shape = new_shape.clone();
        let mut eached = Value::from_row_values(new_values, env)?;
        if is_empty {
            eached.pop_row();
        }
        new_shape.extend_from_slice(&eached.shape().row());
        *eached.shape_mut() = new_shape;
        eached.set_per_meta(per_meta.clone());
        env.push(eached);
    }
    Ok(())
}

pub fn rows(inv: bool, env: &mut Uiua) -> UiuaResult {
    crate::profile_function!();
    let f = env.pop_function()?;
    let sig = f.signature();
    match sig.args {
        0 => env.without_fill(|env| env.call(f)),
        1 => rows1(f, env.pop(1)?, inv, env),
        2 => rows2(f, env.pop(1)?, env.pop(2)?, inv, env),
        n => {
            let mut args = Vec::with_capacity(n);
            for i in 0..n {
                args.push(env.pop(i + 1)?);
            }
            rowsn(f, args, inv, env)
        }
    }
}

pub fn rows1(f: Function, mut xs: Value, inv: bool, env: &mut Uiua) -> UiuaResult {
    if !inv {
        if let Some((f, d)) = f_mon_fast_fn(&f, env) {
            let maybe_through_boxes = matches!(&xs, Value::Box(arr) if arr.rank() <= d + 1);
            if !maybe_through_boxes {
                let val = f(xs, d + 1, env)?;
                env.push(val);
                return Ok(());
            }
        }
        if let Some((f, d)) = f_mon2_fast_fn(&f, env) {
            let maybe_through_boxes = matches!(&xs, Value::Box(arr) if arr.rank() <= d + 1);
            if !maybe_through_boxes {
                let (xs, ys) = f(xs, d + 1, env)?;
                env.push(ys);
                env.push(xs);
                return Ok(());
            }
        }
    }
    let outputs = f.signature().outputs;
    let is_scalar = xs.rank() == 0;
    let is_empty = outputs > 0 && xs.row_count() == 0;
    let mut new_rows = multi_output(
        outputs,
        Vec::with_capacity(xs.row_count() + is_empty as usize),
    );
    let per_meta = xs.take_per_meta();
    env.without_fill(|env| -> UiuaResult {
        if is_empty {
            env.push(xs.proxy_row(env));
            _ = env.call_maintain_sig(f);
            for i in 0..outputs {
                new_rows[i].push(env.pop("rows' function result")?.boxed_if(inv));
            }
        } else {
            for row in xs.into_rows() {
                env.push(row.unboxed_if(inv));
                env.call(f.clone())?;
                for i in 0..outputs {
                    new_rows[i].push(env.pop("rows' function result")?.boxed_if(inv));
                }
            }
        }
        Ok(())
    })?;
    for new_rows in new_rows.into_iter().rev() {
        let mut val = Value::from_row_values(new_rows, env)?;
        if is_scalar {
            val.undo_fix();
        } else if is_empty {
            val.pop_row();
        }
        val.set_per_meta(per_meta.clone());
        env.push(val);
    }

    Ok(())
}

fn rows2(f: Function, mut xs: Value, mut ys: Value, inv: bool, env: &mut Uiua) -> UiuaResult {
    let outputs = f.signature().outputs;
    let both_scalar = xs.rank() == 0 && ys.rank() == 0;
    match (xs.row_count(), ys.row_count()) {
        (_, 1) if !ys.length_is_fillable(env) => {
            ys.undo_fix();
            let is_empty = outputs > 0 && xs.row_count() == 0;
            let mut new_rows = multi_output(outputs, Vec::with_capacity(xs.row_count()));
            let per_meta = xs.take_per_meta();
            env.without_fill(|env| -> UiuaResult {
                if is_empty {
                    env.push(ys.unboxed_if(inv));
                    env.push(xs.proxy_row(env));
                    _ = env.call_maintain_sig(f);
                    for i in 0..outputs {
                        new_rows[i].push(env.pop("rows's function result")?.boxed_if(inv));
                    }
                } else {
                    for x in xs.into_rows() {
                        env.push(ys.clone().unboxed_if(inv));
                        env.push(x.unboxed_if(inv));
                        env.call(f.clone())?;
                        for i in 0..outputs {
                            new_rows[i].push(env.pop("rows's function result")?.boxed_if(inv));
                        }
                    }
                }
                Ok(())
            })?;
            for new_rows in new_rows.into_iter().rev() {
                let mut val = Value::from_row_values(new_rows, env)?;
                if both_scalar {
                    val.undo_fix();
                } else if is_empty {
                    val.pop_row();
                }
                val.set_per_meta(per_meta.clone());
                env.push(val);
            }
            Ok(())
        }
        (1, _) if !xs.length_is_fillable(env) => {
            xs.undo_fix();
            let is_empty = outputs > 0 && ys.row_count() == 0;
            let mut new_rows = multi_output(outputs, Vec::with_capacity(ys.row_count()));
            let per_meta = ys.take_per_meta();
            env.without_fill(|env| -> UiuaResult {
                if is_empty {
                    env.push(ys.proxy_row(env));
                    env.push(xs.unboxed_if(inv));
                    _ = env.call_maintain_sig(f);
                    for i in 0..outputs {
                        new_rows[i].push(env.pop("rows's function result")?.boxed_if(inv));
                    }
                } else {
                    for y in ys.into_rows() {
                        env.push(y.unboxed_if(inv));
                        env.push(xs.clone());
                        env.call(f.clone())?;
                        for i in 0..outputs {
                            new_rows[i].push(env.pop("rows's function result")?.boxed_if(inv));
                        }
                    }
                }
                Ok(())
            })?;
            for new_rows in new_rows.into_iter().rev() {
                let mut val = Value::from_row_values(new_rows, env)?;
                if both_scalar {
                    val.undo_fix();
                } else if is_empty {
                    val.pop_row();
                }
                val.set_per_meta(per_meta.clone());
                env.push(val);
            }
            Ok(())
        }
        (a, b) => {
            if a != b {
                if let Err(e) = xs
                    .fill_length_to(ys.row_count(), env)
                    .and_then(|()| ys.fill_length_to(xs.row_count(), env))
                {
                    return Err(env.error(format!(
                        "Cannot {} arrays with different number of rows {a} and {b}{e}",
                        Primitive::Rows.format(),
                    )));
                }
            }
            if !inv {
                if let Some((f, a, b)) = f_dy_fast_fn(f.instrs(&env.asm), env) {
                    let val = f(xs, ys, a + 1, b + 1, env)?;
                    env.push(val);
                    return Ok(());
                }
            }
            let is_empty = outputs > 0 && (xs.row_count() == 0 || ys.row_count() == 0);
            let mut new_rows = multi_output(
                outputs,
                Vec::with_capacity(xs.row_count() + is_empty as usize),
            );
            let per_meta = xs.take_per_meta().xor(ys.take_per_meta());
            env.without_fill(|env| -> UiuaResult {
                if is_empty {
                    env.push(if ys.row_count() == 0 {
                        ys.proxy_row(env)
                    } else {
                        ys
                    });
                    env.push(if xs.row_count() == 0 {
                        xs.proxy_row(env)
                    } else {
                        xs
                    });
                    _ = env.call_maintain_sig(f);
                    for i in 0..outputs {
                        new_rows[i].push(env.pop("rows's function result")?.boxed_if(inv));
                    }
                } else {
                    for (x, y) in xs.into_rows().zip(ys.into_rows()) {
                        env.push(y.unboxed_if(inv));
                        env.push(x.unboxed_if(inv));
                        env.call(f.clone())?;
                        for i in 0..outputs {
                            new_rows[i].push(env.pop("rows's function result")?.boxed_if(inv));
                        }
                    }
                }
                Ok(())
            })?;
            for new_rows in new_rows.into_iter().rev() {
                let mut val = Value::from_row_values(new_rows, env)?;
                if both_scalar {
                    val.undo_fix();
                } else if is_empty {
                    val.pop_row();
                }
                val.set_per_meta(per_meta.clone());
                env.push(val);
            }
            Ok(())
        }
    }
}

fn rowsn(f: Function, args: Vec<Value>, inv: bool, env: &mut Uiua) -> UiuaResult {
    let outputs = f.signature().outputs;
    let FixedRowsData {
        mut rows,
        row_count,
        is_empty,
        all_scalar,
        per_meta,
    } = fixed_rows(Primitive::Rows.format(), outputs, args, env)?;
    let mut new_values = multi_output(outputs, Vec::new());
    env.without_fill(|env| -> UiuaResult {
        for _ in 0..row_count {
            for arg in rows.iter_mut().rev() {
                match arg {
                    Ok(rows) => env.push(rows.next().unwrap().unboxed_if(inv)),
                    Err(row) => env.push(row.clone().unboxed_if(inv)),
                }
            }
            env.call(f.clone())?;
            for i in 0..outputs {
                new_values[i].push(env.pop("rows's function result")?.boxed_if(inv));
            }
        }
        Ok(())
    })?;
    for new_values in new_values.into_iter().rev() {
        let mut rowsed = Value::from_row_values(new_values, env)?;
        if all_scalar {
            rowsed.undo_fix();
        } else if is_empty {
            rowsed.pop_row();
        }
        rowsed.validate_shape();
        rowsed.set_per_meta(per_meta.clone());
        env.push(rowsed);
    }
    Ok(())
}

pub fn rows_windows(env: &mut Uiua) -> UiuaResult {
    let f = env.pop_function()?;
    if f.signature().args != 1 {
        return Err(env.error(
            "rows windows's function does not take 1 arg. This is a bug in the interpreter",
        ));
    }
    let n_arr = env.pop(1)?;
    let xs = env.pop(2)?;
    if n_arr.rank() != 0 {
        let windows = n_arr.windows(&xs, env)?;
        return rows1(f, windows, false, env);
    }
    let n = n_arr.as_int(env, "Window size must be an integer or list of integers")?;
    let n_abs = n.unsigned_abs();
    if n_abs == 0 {
        return Err(env.error("Window size cannot be zero"));
    }
    let n = n_abs;
    if xs.row_count() < n {
        env.push(xs.first_dim_zero());
        return Ok(());
    }
    if let Some(Primitive::Box) = f.as_primitive(&env.asm) {
        let win_count = xs.row_count() - (n - 1);
        let arr = Array::from_iter(
            (0..win_count).map(|win_start| Boxed(xs.slice_rows(win_start, win_start + n))),
        );
        env.push(arr);
        return Ok(());
    }

    let windows = n_arr.windows(&xs, env)?;
    rows1(f, windows, false, env)
}

impl Value {
    pub(crate) fn length_is_fillable<C>(&self, ctx: &C) -> bool
    where
        C: FillContext,
    {
        if self.rank() == 0 {
            return false;
        }
        match self {
            Value::Num(_) => ctx.scalar_fill::<f64>().is_ok(),
            Value::Byte(_) => ctx.scalar_fill::<u8>().is_ok(),
            Value::Complex(_) => ctx.scalar_fill::<Complex>().is_ok(),
            Value::Char(_) => ctx.scalar_fill::<char>().is_ok(),
            Value::Box(_) => ctx.scalar_fill::<Boxed>().is_ok(),
        }
    }
    pub(crate) fn fill_length_to<C>(&mut self, len: usize, ctx: &C) -> Result<(), &'static str>
    where
        C: FillContext,
    {
        match self {
            Value::Num(arr) => arr.fill_length_to(len, ctx),
            Value::Byte(arr) => arr.fill_length_to(len, ctx),
            Value::Complex(arr) => arr.fill_length_to(len, ctx),
            Value::Char(arr) => arr.fill_length_to(len, ctx),
            Value::Box(arr) => arr.fill_length_to(len, ctx),
        }
    }
}

impl<T: ArrayValue> Array<T> {
    pub(crate) fn fill_length_to<C>(&mut self, len: usize, ctx: &C) -> Result<(), &'static str>
    where
        C: FillContext,
    {
        if self.rank() == 0 || self.row_count() >= len {
            return Ok(());
        }
        let fill = ctx.scalar_fill::<T>()?;
        let more_elems = (len - self.row_count()) * self.row_len();
        self.data.reserve(more_elems);
        self.data.extend_repeat(&fill, more_elems);
        self.shape[0] = len;
        Ok(())
    }
}
