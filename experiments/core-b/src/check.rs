//! One direct expression-checking authority for the deliberately bounded Core B subset.
//! Param denotes a declaration parameter, never a solver variable. Template HIR is
//! validated and discarded; only complete concrete instances enter Program.functions.
use biomiya::{
    diagnostic::{Diagnostic, Result},
    hir::{self as h, ExprKind as H},
    ir::ScopeId,
    resolve::{Namespace, Resolver},
    source::Span,
    syntax::{self as a, ExprKind as A},
    types::{Abi, DefId, FnType, Integer, TyId, TypeContext, TypeKind as T},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_INSTANCES: usize = 128;
const MAX_INSTANCE_DEPTH: usize = 32;
const MAX_EXPRESSION_DEPTH: usize = 32;

fn error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_TYPE", span, message)
}
fn unsupported(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_CORE_B_UNSUPPORTED", span, message)
}

#[derive(Clone)]
struct Definition {
    name: String,
    function: a::Function,
    signature: FnType,
    span: Span,
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Instance {
    definition: DefId,
    arguments: Vec<TyId>,
}
#[derive(Clone, Copy)]
enum InstanceState {
    Pending,
    Active,
    Ready,
}
struct Body {
    template: bool,
    parameters: BTreeMap<String, TyId>,
    copy_parameters: BTreeSet<u32>,
    bindings: Vec<BTreeMap<String, h::LocalId>>,
    locals: Vec<h::Local>,
    result: TyId,
    scope: ScopeId,
    next_scope: u32,
    depth: usize,
}

struct Checker {
    types: TypeContext,
    resolver: Resolver,
    definitions: BTreeMap<DefId, Definition>,
    instances: BTreeMap<Instance, InstanceState>,
    current: Option<Instance>,
    pending: VecDeque<Instance>,
    dependencies: BTreeMap<Instance, BTreeSet<Instance>>,
    functions: Vec<h::Function>,
}

/// Check all declared bodies, then materialize all ordinary functions and their
/// demanded explicit generic instances. No call to the production checker occurs.
pub fn check(files: &[a::AstFile]) -> Result<h::Program> {
    if files.len() != 1 {
        return Err(unsupported(
            Span::default(),
            "only one source file is supported",
        ));
    }
    let file = &files[0];
    if !file.module.is_empty() || !file.imports.is_empty() {
        return Err(unsupported(
            file.declarations
                .first()
                .map_or(Span::default(), |d| d.span),
            "module and import declarations are outside this prototype",
        ));
    }
    let resolver = Resolver::new(files)?;
    let mut checker = Checker {
        types: TypeContext::new(),
        resolver,
        definitions: BTreeMap::new(),
        instances: BTreeMap::new(),
        current: None,
        pending: VecDeque::new(),
        dependencies: BTreeMap::new(),
        functions: vec![],
    };
    checker.collect()?;
    // Definition checks cannot depend on whether a call happens to request an instance.
    for (id, definition) in checker.definitions.clone() {
        let arguments = (0..definition.function.generics.len())
            .map(|i| checker.types.intern(T::Param(i as u32)))
            .collect::<Vec<_>>();
        checker.check_body(id, &arguments, true)?;
    }
    for (id, definition) in checker.definitions.clone() {
        if definition.function.generics.is_empty() {
            checker.materialize(id, &[], definition.span)?;
        }
    }
    checker.finish_instances()?;
    Ok(h::Program {
        declarations: Some(checker.resolver),
        types: checker.types,
        functions: checker.functions,
        data: vec![],
        externals: vec![],
    })
}

impl Checker {
    fn collect(&mut self) -> Result<()> {
        for (id, record) in self.resolver.declarations.clone() {
            let declaration = record.decl;
            let a::DeclKind::Function(function) = declaration.kind else {
                return Err(unsupported(
                    declaration.span,
                    "only function declarations are supported",
                ));
            };
            if !declaration.attributes.is_empty()
                || function.unsafe_
                || function.external
                || !function.effects.is_empty()
                || function.body.is_none()
            {
                return Err(unsupported(
                    declaration.span,
                    "attributes, extern/unsafe/effects and bodyless functions are outside this prototype",
                ));
            }
            let mut parameters = BTreeMap::new();
            for (index, parameter) in function.generics.iter().enumerate() {
                if parameters
                    .insert(
                        parameter.name.clone(),
                        self.types.intern(T::Param(index as u32)),
                    )
                    .is_some()
                {
                    return Err(Diagnostic::new(
                        "E_NAME",
                        parameter.span,
                        "duplicate generic parameter",
                    ));
                }
            }
            let mut names = BTreeSet::new();
            let mut params = vec![];
            for parameter in &function.parameters {
                if parameter.receiver.is_some() {
                    return Err(unsupported(
                        parameter.span,
                        "method receivers are outside this prototype",
                    ));
                }
                if !names.insert(&parameter.name) {
                    return Err(Diagnostic::new(
                        "E_NAME",
                        parameter.span,
                        "duplicate parameter",
                    ));
                }
                let ty = self.resolve_type(&parameter.ty, &parameters)?;
                self.object_type(ty, parameter.span)?;
                params.push(ty);
            }
            let result = self.resolve_type(&function.result, &parameters)?;
            self.definitions.insert(
                id,
                Definition {
                    name: declaration.name,
                    signature: FnType {
                        params,
                        result,
                        abi: Abi::Biomiya,
                        unsafe_: false,
                        effects: BTreeSet::new(),
                    },
                    function,
                    span: declaration.span,
                },
            );
        }
        Ok(())
    }

    fn unit(&mut self) -> TyId {
        self.types.intern(T::Unit)
    }
    fn bool(&mut self) -> TyId {
        self.types.intern(T::Bool)
    }
    fn never(&mut self) -> TyId {
        self.types.intern(T::Never)
    }

    fn resolve_type(&mut self, ty: &a::Type, parameters: &BTreeMap<String, TyId>) -> Result<TyId> {
        let kind = match &ty.kind {
            a::TypeKind::Tuple(elements) => {
                if elements.is_empty() {
                    T::Unit
                } else {
                    let fields = elements
                        .iter()
                        .map(|t| self.resolve_type(t, parameters))
                        .collect::<Result<Vec<_>>>()?;
                    for field in &fields {
                        self.object_type(*field, ty.span)?;
                    }
                    T::Tuple(fields)
                }
            }
            a::TypeKind::Path(path) if path.len() == 1 => {
                let segment = &path[0];
                if let Some(parameter) = parameters.get(&segment.name) {
                    if !segment.arguments.is_empty() {
                        return Err(error(
                            segment.span,
                            "a type parameter cannot have type arguments",
                        ));
                    }
                    return Ok(*parameter);
                }
                if let Some(integer) = integer_type(&segment.name) {
                    if !segment.arguments.is_empty() {
                        return Err(error(
                            segment.span,
                            "primitive type cannot have type arguments",
                        ));
                    }
                    T::Int(integer)
                } else if segment.name == "bool" {
                    if !segment.arguments.is_empty() {
                        return Err(error(segment.span, "bool cannot have type arguments"));
                    }
                    T::Bool
                } else if segment.name == "Option" {
                    if segment.arguments.len() != 1 {
                        return Err(error(
                            segment.span,
                            "Option requires exactly one type argument",
                        ));
                    }
                    let inner = self.resolve_type(&segment.arguments[0], parameters)?;
                    self.object_type(inner, segment.span)?;
                    T::Option(inner)
                } else {
                    let names = vec![segment.name.clone()];
                    if self
                        .resolver
                        .builtin_path(&[], &names, Namespace::Type, ty.span)?
                        .is_some()
                    {
                        return Err(unsupported(
                            ty.span,
                            format!(
                                "builtin type {} is outside the supported subset",
                                segment.name
                            ),
                        ));
                    }
                    self.resolver
                        .lookup_visible(&[], &names, Namespace::Type, ty.span)?;
                    return Err(unsupported(
                        ty.span,
                        "user nominal and alias types are outside this prototype",
                    ));
                }
            }
            a::TypeKind::Never => T::Never,
            a::TypeKind::Function {
                parameters: args,
                result,
                unsafe_,
                external,
                effects,
            } => {
                if *unsafe_ || *external || !effects.is_empty() {
                    return Err(unsupported(
                        ty.span,
                        "function pointer adaptations and ABI/effects are unsupported",
                    ));
                }
                let params = args
                    .iter()
                    .map(|t| self.resolve_type(t, parameters))
                    .collect::<Result<Vec<_>>>()?;
                for p in &params {
                    self.object_type(*p, ty.span)?;
                }
                let result = self.resolve_type(result, parameters)?;
                T::Function(FnType {
                    params,
                    result,
                    abi: Abi::Biomiya,
                    unsafe_: false,
                    effects: BTreeSet::new(),
                })
            }
            _ => {
                return Err(unsupported(
                    ty.span,
                    "reference/pointer/slice/array/nominal/qualified types are outside this prototype",
                ));
            }
        };
        Ok(self.types.intern(kind))
    }

    fn object_type(&self, ty: TyId, span: Span) -> Result<()> {
        if matches!(self.types.kind(ty), T::Never) {
            Err(error(span, "never cannot be an object or generic argument"))
        } else {
            Ok(())
        }
    }

    fn has_parameter(&self, ty: TyId) -> bool {
        match self.types.kind(ty) {
            T::Param(_) => true,
            T::Tuple(fields) => fields.iter().any(|t| self.has_parameter(*t)),
            T::Option(inner) => self.has_parameter(*inner),
            T::Function(f) => {
                f.params.iter().any(|t| self.has_parameter(*t)) || self.has_parameter(f.result)
            }
            _ => false,
        }
    }

    fn signature(&mut self, definition: DefId, arguments: &[TyId]) -> FnType {
        let template = self.definitions[&definition].signature.clone();
        FnType {
            params: template
                .params
                .iter()
                .map(|t| self.types.substitute(*t, arguments))
                .collect(),
            result: self.types.substitute(template.result, arguments),
            ..template
        }
    }

    fn symbol(&self, definition: DefId, arguments: &[TyId]) -> String {
        let record = &self.definitions[&definition];
        let mut symbol = format!("__core_b_{}_{}", record.name, definition.0);
        for arg in arguments {
            symbol.push_str(&format!("_{}", arg.0));
        }
        symbol
    }

    fn materialize(&mut self, definition: DefId, arguments: &[TyId], span: Span) -> Result<String> {
        let key = Instance {
            definition,
            arguments: arguments.to_vec(),
        };
        if arguments.iter().any(|t| self.has_parameter(*t)) {
            return Err(error(
                span,
                "concrete instance contains a template parameter",
            ));
        }
        // Add and verify every dependency, including edges to already cached keys.
        // Requests only enqueue bodies: checking a call cannot recursively consume
        // the compiler's host stack by checking the callee's body.
        if let Some(caller) = &self.current {
            self.dependencies
                .entry(caller.clone())
                .or_default()
                .insert(key.clone());
            self.check_instance_dependencies(span)?;
        }
        if !self.instances.contains_key(&key) {
            if self.instances.len() >= MAX_INSTANCES {
                return Err(unsupported(span, "prototype instance count limit exceeded"));
            }
            self.instances.insert(key.clone(), InstanceState::Pending);
            self.pending.push_back(key);
        }
        Ok(self.symbol(definition, arguments))
    }

    fn finish_instances(&mut self) -> Result<()> {
        while let Some(key) = self.pending.pop_front() {
            self.instances.insert(key.clone(), InstanceState::Active);
            self.current = Some(key.clone());
            let result = self.check_body(key.definition, &key.arguments, false);
            self.current = None;
            let function = result.map_err(|error| {
                error.note(
                    None,
                    format!(
                        "instantiating {}",
                        self.symbol(key.definition, &key.arguments)
                    ),
                )
            })?;
            self.instances.insert(key, InstanceState::Ready);
            self.functions.push(function);
        }
        Ok(())
    }

    fn check_instance_dependencies(&self, span: Span) -> Result<()> {
        // The graph has a hard node bound. Iterative reachability from each root
        // detects any same-declaration/different-argument path, independent of
        // body ordering and Pending/Active/Ready cache state.
        for root in self.dependencies.keys() {
            let mut seen = BTreeSet::new();
            let mut work = VecDeque::from([(root.clone(), 0usize)]);
            while let Some((node, depth)) = work.pop_front() {
                if !seen.insert(node.clone()) {
                    continue;
                }
                if node.definition == root.definition && node.arguments != root.arguments {
                    return Err(Diagnostic::new(
                        "E_INSTANTIATION",
                        span,
                        "polymorphic recursive instance is forbidden",
                    )
                    .note(
                        None,
                        format!(
                            "{} depends on {}",
                            self.symbol(root.definition, &root.arguments),
                            self.symbol(node.definition, &node.arguments)
                        ),
                    ));
                }
                if depth >= MAX_INSTANCE_DEPTH {
                    return Err(unsupported(
                        span,
                        "prototype instance dependency depth limit exceeded",
                    ));
                }
                if let Some(children) = self.dependencies.get(&node) {
                    work.extend(children.iter().map(|child| (child.clone(), depth + 1)));
                }
            }
        }
        Ok(())
    }

    fn check_body(
        &mut self,
        definition: DefId,
        arguments: &[TyId],
        template: bool,
    ) -> Result<h::Function> {
        let record = self.definitions[&definition].clone();
        let signature = self.signature(definition, arguments);
        let parameters = record
            .function
            .generics
            .iter()
            .zip(arguments)
            .map(|(g, t)| (g.name.clone(), *t))
            .collect();
        let copy_parameters = record
            .function
            .generics
            .iter()
            .enumerate()
            .filter_map(|(i, g)| g.copy.then_some(i as u32))
            .collect();
        let mut body = Body {
            template,
            parameters,
            copy_parameters,
            bindings: vec![BTreeMap::new()],
            locals: vec![],
            result: signature.result,
            scope: ScopeId(u32::MAX),
            next_scope: 0,
            depth: 0,
        };
        let mut locals = vec![];
        for (parameter, ty) in record.function.parameters.iter().zip(&signature.params) {
            locals.push(self.bind(&mut body, &parameter.name, *ty, parameter.span)?);
        }
        let block = a::Expr {
            kind: A::Block(record.function.body.clone().unwrap()),
            span: record.span,
        };
        let value = self.check_expr(&block, Some(signature.result), &mut body)?;
        if !template {
            for local in &body.locals {
                if self.has_parameter(local.ty) {
                    return Err(error(
                        local.span,
                        "concrete local retains a template parameter",
                    ));
                }
            }
        }
        Ok(h::Function {
            name: record.name,
            definition,
            symbol: self.symbol(definition, arguments),
            signature,
            type_arguments: arguments.to_vec(),
            parameters: locals,
            locals: body.locals,
            body: value,
            span: record.span,
            special: None,
            exported: false,
            section: None,
        })
    }

    fn bind(&mut self, body: &mut Body, name: &str, ty: TyId, span: Span) -> Result<h::LocalId> {
        if body.bindings.last().unwrap().contains_key(name) {
            return Err(Diagnostic::new("E_NAME", span, "duplicate local binding"));
        }
        let id = h::LocalId(body.locals.len() as u32);
        body.locals.push(h::Local {
            name: name.into(),
            ty,
            mutable: false,
            scope: body.scope,
            span,
        });
        body.bindings.last_mut().unwrap().insert(name.into(), id);
        Ok(id)
    }

    fn check_expected(&self, value: &h::Expr, expected: Option<TyId>, body: &Body) -> Result<()> {
        if let Some(target) = expected {
            if value.ty != target && !matches!(self.types.kind(value.ty), T::Never) {
                if body.template && (self.has_parameter(value.ty) || self.has_parameter(target)) {
                    return Err(unsupported(
                        value.span,
                        "parameter-dependent type relationship is outside the supported template rules",
                    ));
                }
                return Err(error(
                    value.span,
                    format!(
                        "expected {}, found {}",
                        self.types.display(target),
                        self.types.display(value.ty)
                    ),
                ));
            }
        }
        Ok(())
    }

    fn same_type(&self, left: TyId, right: TyId, span: Span, body: &Body) -> Result<TyId> {
        if matches!(self.types.kind(left), T::Never) {
            return Ok(right);
        }
        if matches!(self.types.kind(right), T::Never) {
            return Ok(left);
        }
        if left == right {
            return Ok(left);
        }
        if body.template && (self.has_parameter(left) || self.has_parameter(right)) {
            return Err(unsupported(
                span,
                "parameter-dependent branch relation is unsupported",
            ));
        }
        Err(error(
            span,
            format!(
                "independent results differ: {} and {}",
                self.types.display(left),
                self.types.display(right)
            ),
        ))
    }

    /// The only expression typing entry. Every recursive expression uses this
    /// method and the expected type provided before that child is checked.
    fn check_expr(
        &mut self,
        node: &a::Expr,
        expected: Option<TyId>,
        body: &mut Body,
    ) -> Result<h::Expr> {
        if body.depth >= MAX_EXPRESSION_DEPTH {
            return Err(unsupported(
                node.span,
                "prototype expression nesting limit exceeded",
            ));
        }
        body.depth += 1;
        let result = (|| {
            let span = node.span;
            let (ty, kind) = match &node.kind {
                A::Integer(text) => self.literal(text, false, expected, span)?,
                A::Bool(value) => (self.bool(), H::Integer(*value as u64)),
                A::Tuple(elements) => {
                    if elements.is_empty() {
                        (self.unit(), H::Unit)
                    } else {
                        let expected_fields = expected.and_then(|t| match self.types.kind(t) {
                            T::Tuple(fields) => Some(fields.clone()),
                            _ => None,
                        });
                        if expected_fields
                            .as_ref()
                            .is_some_and(|fields| fields.len() != elements.len())
                        {
                            return Err(error(span, "tuple arity does not match expectation"));
                        }
                        let mut fields = vec![];
                        for (index, element) in elements.iter().enumerate() {
                            let target = expected_fields.as_ref().map(|fields| fields[index]);
                            let value = self.check_expr(element, target, body)?;
                            // An all-diverging tuple still has no runtime value. To
                            // retain object types, require a known type for its component.
                            if matches!(self.types.kind(value.ty), T::Never) && target.is_none() {
                                return Err(unsupported(
                                    element.span,
                                    "diverging tuple element without known component type",
                                ));
                            }
                            fields.push((index, value));
                        }
                        let component_types = fields
                            .iter()
                            .map(|(i, e)| expected_fields.as_ref().map_or(e.ty, |f| f[*i]))
                            .collect();
                        let ty = self.types.intern(T::Tuple(component_types));
                        (
                            ty,
                            H::Aggregate {
                                variant: None,
                                fields,
                            },
                        )
                    }
                }
                A::Path(path) => return self.path_value(path, expected, span, body),
                A::Call { callee, arguments } => {
                    if let A::Path(path) = &callee.kind {
                        if let Some(variant) = self.option_constructor(path, span, body)? {
                            return self
                                .construct_option(path, variant, arguments, expected, span, body);
                        }
                    }
                    let callee = self.check_expr(callee, None, body)?;
                    let T::Function(signature) = self.types.kind(callee.ty).clone() else {
                        return Err(error(span, "call requires a function value"));
                    };
                    if signature.params.len() != arguments.len() {
                        return Err(error(span, "wrong value argument count"));
                    }
                    let arguments = arguments
                        .iter()
                        .zip(&signature.params)
                        .map(|(arg, ty)| self.check_expr(arg, Some(*ty), body))
                        .collect::<Result<Vec<_>>>()?;
                    (
                        signature.result,
                        H::Call {
                            callee: Box::new(callee),
                            signature,
                            arguments,
                        },
                    )
                }
                A::Field {
                    base,
                    name,
                    arguments,
                    ..
                } => {
                    if !arguments.is_empty() {
                        return Err(unsupported(
                            span,
                            "method/type-argument field access is outside this prototype",
                        ));
                    }
                    let base = self.check_expr(base, None, body)?;
                    let fields = match self.types.kind(base.ty) {
                        T::Tuple(fields) => fields.clone(),
                        T::Param(_) => {
                            return Err(unsupported(
                                span,
                                "parameter-dependent field access is unsupported",
                            ));
                        }
                        _ => return Err(error(span, "tuple field requires a tuple base")),
                    };
                    let index = name
                        .parse::<usize>()
                        .ok()
                        .filter(|i| *i < fields.len())
                        .ok_or_else(|| error(span, "tuple field does not exist"))?;
                    (
                        fields[index],
                        H::Field {
                            base: Box::new(base),
                            field: index,
                        },
                    )
                }
                A::Block(block) => {
                    let previous_scope = body.scope;
                    body.scope = ScopeId(body.next_scope);
                    body.next_scope += 1;
                    let scope = body.scope;
                    body.bindings.push(BTreeMap::new());
                    let mut statements = vec![];
                    let mut normal = true;
                    for statement in &block.statements {
                        match statement {
                            a::Statement::Empty(_) => {}
                            a::Statement::Binding {
                                mutable,
                                pattern,
                                ty,
                                value,
                                ..
                            } => {
                                if *mutable {
                                    return Err(unsupported(
                                        pattern.span,
                                        "var and assignment are outside this prototype",
                                    ));
                                }
                                let a::PatternKind::Path(names) = &pattern.kind else {
                                    return Err(unsupported(
                                        pattern.span,
                                        "only single-name let bindings are supported",
                                    ));
                                };
                                if names.len() != 1 {
                                    return Err(unsupported(
                                        pattern.span,
                                        "only single-name let bindings are supported",
                                    ));
                                }
                                let target = ty
                                    .as_ref()
                                    .map(|t| self.resolve_type(t, &body.parameters))
                                    .transpose()?;
                                let value = self.check_expr(value, target, body)?;
                                normal &= !matches!(self.types.kind(value.ty), T::Never);
                                let ty = target.unwrap_or(value.ty);
                                self.object_type(ty, pattern.span)?;
                                let local = self.bind(body, &names[0], ty, pattern.span)?;
                                statements.push(h::Statement::Bind {
                                    pattern: h::Pattern {
                                        ty,
                                        span: pattern.span,
                                        kind: h::PatternKind::Bind(local),
                                    },
                                    value,
                                });
                            }
                            a::Statement::Expr(expression) => {
                                let expression = self.check_expr(expression, None, body)?;
                                normal &= !matches!(self.types.kind(expression.ty), T::Never);
                                statements.push(h::Statement::Expr(expression));
                            }
                            a::Statement::Assert(expression) => {
                                return Err(unsupported(
                                    expression.span,
                                    "static_assert/T2 dependency checks are not implemented",
                                ));
                            }
                            a::Statement::Defer(expression) => {
                                return Err(unsupported(
                                    expression.span,
                                    "defer is outside this prototype",
                                ));
                            }
                        }
                    }
                    let tail = block
                        .tail
                        .as_ref()
                        .map(|tail| self.check_expr(tail, expected, body).map(Box::new))
                        .transpose()?;
                    let ty = if !normal {
                        self.never()
                    } else {
                        tail.as_ref().map_or_else(|| self.unit(), |tail| tail.ty)
                    };
                    body.bindings.pop();
                    body.scope = previous_scope;
                    (
                        ty,
                        H::Block {
                            scope,
                            statements,
                            tail,
                        },
                    )
                }
                A::If {
                    condition,
                    then,
                    else_,
                } => {
                    let bool_ = self.bool();
                    let condition = self.check_expr(condition, Some(bool_), body)?;
                    let then_node = a::Expr {
                        kind: A::Block(then.clone()),
                        span: then.span,
                    };
                    let branch_expectation = if else_.is_none() {
                        Some(self.unit())
                    } else {
                        expected
                    };
                    let then = self.check_expr(&then_node, branch_expectation, body)?;
                    let else_ = else_
                        .as_ref()
                        .map(|branch| self.check_expr(branch, expected, body).map(Box::new))
                        .transpose()?;
                    let ty = if let Some(other) = &else_ {
                        self.same_type(then.ty, other.ty, span, body)?
                    } else {
                        self.unit()
                    };
                    (
                        ty,
                        H::If {
                            condition: Box::new(condition),
                            then: Box::new(then),
                            else_,
                        },
                    )
                }
                A::Unary {
                    op,
                    operand,
                    adjacent,
                } => {
                    if op == "-" && *adjacent {
                        if let A::Integer(text) = &operand.kind {
                            return self.literal(text, true, expected, span).and_then(
                                |(ty, kind)| {
                                    let value = h::Expr { ty, kind, span };
                                    self.check_expected(&value, expected, body)?;
                                    Ok(value)
                                },
                            );
                        }
                    }
                    let target = if op == "!" {
                        Some(self.bool())
                    } else {
                        expected.filter(|ty| matches!(self.types.kind(*ty), T::Int(_)))
                    };
                    if !["!", "~", "-"].contains(&op.as_str()) {
                        return Err(unsupported(
                            span,
                            "borrow, dereference and other unary operations are unsupported",
                        ));
                    }
                    let value = self.check_expr(operand, target, body)?;
                    match (op.as_str(), self.types.kind(value.ty)) {
                        ("!", T::Bool) | ("~", T::Int(_)) => {}
                        ("-", T::Int(integer)) if integer.signed() => {}
                        (_, T::Param(_)) => {
                            return Err(unsupported(
                                span,
                                "parameter-dependent unary capability is unsupported",
                            ));
                        }
                        _ => {
                            return Err(error(
                                span,
                                "unsupported operand type for unary operation",
                            ));
                        }
                    }
                    (
                        value.ty,
                        H::Unary {
                            op: op.clone(),
                            value: Box::new(value),
                        },
                    )
                }
                A::Binary { op, left, right } => {
                    let logical = ["&&", "||"].contains(&op.as_str());
                    let comparison = ["==", "!=", "<", "<=", ">", ">="].contains(&op.as_str());
                    let arithmetic = [
                        "+", "-", "*", "/", "%", "+%", "-%", "*%", "&", "|", "^", "<<", ">>",
                    ]
                    .contains(&op.as_str());
                    let checked = ["+?", "-?", "*?"].contains(&op.as_str());
                    if !(logical || comparison || arithmetic || checked) {
                        return Err(unsupported(
                            span,
                            format!("binary operation {op} is unsupported"),
                        ));
                    }
                    let operand_target = if logical {
                        Some(self.bool())
                    } else if comparison {
                        None
                    } else if checked {
                        expected.and_then(|t| match self.types.kind(t) {
                            T::Option(i) if matches!(self.types.kind(*i), T::Int(_)) => Some(*i),
                            _ => None,
                        })
                    } else {
                        expected.filter(|t| matches!(self.types.kind(*t), T::Int(_)))
                    };
                    let left = self.check_expr(left, operand_target, body)?;
                    // The language's explicit shift-count literal rule is local to
                    // shift. It does not infer arbitrary right expressions.
                    let right_target = if ["<<", ">>"].contains(&op.as_str()) {
                        matches!(&right.kind, A::Integer(text) if literal_suffix(text).is_none())
                            .then_some(left.ty)
                    } else {
                        operand_target
                    };
                    let right = self.check_expr(right, right_target, body)?;
                    if left.ty != right.ty {
                        return Err(error(
                            span,
                            "binary operands must independently have the same type",
                        ));
                    }
                    let operand_kind = self.types.kind(left.ty);
                    if self.has_parameter(left.ty) {
                        return Err(unsupported(
                            span,
                            "parameter-dependent binary capability is unsupported",
                        ));
                    }
                    let supported = if logical {
                        matches!(operand_kind, T::Bool)
                    } else if comparison && ["==", "!="].contains(&op.as_str()) {
                        matches!(operand_kind, T::Int(_) | T::Bool | T::Unit)
                    } else {
                        matches!(operand_kind, T::Int(_))
                    };
                    if !supported {
                        if comparison
                            && matches!(operand_kind, T::Tuple(_) | T::Option(_) | T::Function(_))
                        {
                            return Err(unsupported(
                                span,
                                "aggregate/function equality is outside this prototype",
                            ));
                        }
                        return Err(error(span, "operand type does not support this operation"));
                    }
                    let ty = if comparison || logical {
                        self.bool()
                    } else if checked {
                        self.types.intern(T::Option(left.ty))
                    } else {
                        left.ty
                    };
                    (
                        ty,
                        H::Binary {
                            op: op.clone(),
                            left: Box::new(left),
                            right: Box::new(right),
                        },
                    )
                }
                A::Cast { value, ty } => {
                    let value = self.check_expr(value, None, body)?;
                    let target = self.resolve_type(ty, &body.parameters)?;
                    if self.has_parameter(value.ty) || self.has_parameter(target) {
                        return Err(unsupported(span, "parameter-dependent cast is unsupported"));
                    }
                    if !matches!(self.types.kind(value.ty), T::Int(_))
                        || !matches!(self.types.kind(target), T::Int(_))
                    {
                        return Err(unsupported(span, "only integer casts are supported"));
                    }
                    (target, H::Cast(Box::new(value)))
                }
                A::Return(value) => {
                    let value = value
                        .as_ref()
                        .map(|value| {
                            self.check_expr(value, Some(body.result), body)
                                .map(Box::new)
                        })
                        .transpose()?;
                    if value.is_none() && !matches!(self.types.kind(body.result), T::Unit) {
                        return Err(error(span, "bare return requires unit result"));
                    }
                    (self.never(), H::Return(value))
                }
                A::Null => {
                    return Err(unsupported(
                        span,
                        "raw pointer/null checking is outside this prototype",
                    ));
                }
                A::Array(_) | A::Repeat { .. } => {
                    return Err(unsupported(
                        span,
                        "arrays and T2 length dependency checks are not implemented",
                    ));
                }
                A::Layout { .. } => {
                    return Err(unsupported(
                        span,
                        "source layout queries are outside the P prototype",
                    ));
                }
                _ => {
                    return Err(unsupported(
                        span,
                        "expression syntax is outside the P prototype",
                    ));
                }
            };
            let value = h::Expr { ty, kind, span };
            self.check_expected(&value, expected, body)?;
            Ok(value)
        })();
        body.depth -= 1;
        result
    }

    fn path_value(
        &mut self,
        path: &[a::PathSegment],
        expected: Option<TyId>,
        span: Span,
        body: &mut Body,
    ) -> Result<h::Expr> {
        if path.len() == 1 {
            if let Some(local) = body
                .bindings
                .iter()
                .rev()
                .find_map(|scope| scope.get(&path[0].name))
                .copied()
            {
                if !path[0].arguments.is_empty() {
                    return Err(error(span, "a local binding cannot have type arguments"));
                }
                let value = h::Expr {
                    ty: body.locals[local.0 as usize].ty,
                    kind: H::Local(local),
                    span,
                };
                self.check_expected(&value, expected, body)?;
                return Ok(value);
            }
        }
        if let Some(variant) = self.option_constructor(path, span, body)? {
            if variant == 0 {
                return Err(error(span, "Some constructor needs one payload"));
            }
            return self.construct_option(path, variant, &[], expected, span, body);
        }
        let names = path.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        if self
            .resolver
            .builtin_path(&[], &names, Namespace::Value, span)?
            .is_some()
        {
            return Err(unsupported(
                span,
                "builtin functions outside Option construction are unsupported",
            ));
        }
        let definition = self
            .resolver
            .lookup_visible(&[], &names, Namespace::Value, span)?;
        let record = self
            .definitions
            .get(&definition)
            .ok_or_else(|| unsupported(span, "only ordinary function values are supported"))?
            .clone();
        if path.len() != 1 {
            return Err(unsupported(
                span,
                "qualified user function paths are unsupported",
            ));
        }
        let arguments = path[0]
            .arguments
            .iter()
            .map(|t| self.resolve_type(t, &body.parameters))
            .collect::<Result<Vec<_>>>()?;
        if arguments.len() != record.function.generics.len() {
            return Err(error(
                span,
                "generic function paths require every type argument explicitly; non-generic paths accept none",
            ));
        }
        for (arg, generic) in arguments.iter().zip(&record.function.generics) {
            self.object_type(*arg, span)?;
            if generic.copy && !self.copy_capability(*arg, body) {
                if self.has_parameter(*arg) {
                    return Err(unsupported(span, "unsupported template Copy obligation"));
                }
                return Err(Diagnostic::new(
                    "E_INSTANTIATION",
                    span,
                    "type argument violates Copy constraint",
                ));
            }
        }
        let signature = self.signature(definition, &arguments);
        let symbol = if body.template {
            self.symbol(definition, &arguments)
        } else {
            self.materialize(definition, &arguments, span)?
        };
        let ty = self.types.intern(T::Function(signature));
        let value = h::Expr {
            ty,
            kind: H::Function { symbol },
            span,
        };
        self.check_expected(&value, expected, body)?;
        Ok(value)
    }

    fn copy_capability(&mut self, ty: TyId, body: &Body) -> bool {
        match self.types.kind(ty).clone() {
            T::Param(index) => body.copy_parameters.contains(&index),
            T::Tuple(fields) => fields.into_iter().all(|t| self.copy_capability(t, body)),
            T::Option(inner) => self.copy_capability(inner, body),
            _ => self.types.properties(ty).copy,
        }
    }

    fn option_constructor(
        &self,
        path: &[a::PathSegment],
        span: Span,
        body: &Body,
    ) -> Result<Option<usize>> {
        let names = path.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        if names.len() == 1
            && body
                .bindings
                .iter()
                .rev()
                .any(|scope| scope.contains_key(&names[0]))
        {
            return Ok(None);
        }
        // Resolver identity, not spelling: a user function called Some shadows the
        // preimported constructor and follows the normal explicit-function rule.
        let builtin = self
            .resolver
            .builtin_path(&[], &names, Namespace::Value, span)?;
        Ok(match builtin.as_deref() {
            Some([core, variant]) if core == "core" && variant == "None" => Some(1),
            Some([core, variant]) if core == "core" && variant == "Some" => Some(0),
            Some([core, option, variant])
                if core == "core" && option == "Option" && variant == "None" =>
            {
                Some(1)
            }
            Some([core, option, variant])
                if core == "core" && option == "Option" && variant == "Some" =>
            {
                Some(0)
            }
            _ => None,
        })
    }

    fn construct_option(
        &mut self,
        path: &[a::PathSegment],
        variant: usize,
        arguments: &[a::Expr],
        expected: Option<TyId>,
        span: Span,
        body: &mut Body,
    ) -> Result<h::Expr> {
        if arguments.len() != usize::from(variant == 0) {
            return Err(error(span, "wrong Option constructor payload count"));
        }
        let mut explicit = vec![];
        for segment in path {
            if !segment.arguments.is_empty() && segment.name != "Option" {
                return Err(error(
                    segment.span,
                    "constructor type arguments belong on Option",
                ));
            }
            explicit.extend(segment.arguments.iter());
        }
        let inner = if !explicit.is_empty() {
            if explicit.len() != 1 {
                return Err(error(
                    span,
                    "Option constructor requires exactly one explicit type argument",
                ));
            }
            self.resolve_type(explicit[0], &body.parameters)?
        } else {
            match expected.map(|t| self.types.kind(t)) {
                Some(T::Option(inner)) if !self.has_parameter(*inner) => *inner,
                _ => {
                    return Err(error(
                        span,
                        "constructor omission requires a complete concrete expected Option instance",
                    ));
                }
            }
        };
        self.object_type(inner, span)?;
        let fields = arguments
            .iter()
            .enumerate()
            .map(|(i, arg)| self.check_expr(arg, Some(inner), body).map(|e| (i, e)))
            .collect::<Result<Vec<_>>>()?;
        let ty = self.types.intern(T::Option(inner));
        let value = h::Expr {
            ty,
            kind: H::Aggregate {
                variant: Some(variant),
                fields,
            },
            span,
        };
        self.check_expected(&value, expected, body)?;
        Ok(value)
    }

    fn literal(
        &mut self,
        text: &str,
        negative: bool,
        expected: Option<TyId>,
        span: Span,
    ) -> Result<(TyId, H)> {
        let suffix = literal_suffix(text);
        let digits = suffix
            .map_or(text, |(_, name)| {
                text.strip_suffix(&format!("_{name}")).unwrap()
            })
            .replace('_', "");
        let (radix, digits) = if let Some(d) = digits.strip_prefix("0x") {
            (16, d)
        } else if let Some(d) = digits.strip_prefix("0b") {
            (2, d)
        } else {
            (10, digits.as_str())
        };
        let magnitude = u128::from_str_radix(digits, radix)
            .map_err(|_| error(span, "invalid or oversized integer literal"))?;
        let integer = suffix
            .map(|(integer, _)| integer)
            .or_else(|| {
                expected.and_then(|t| match self.types.kind(t) {
                    T::Int(integer) => Some(*integer),
                    _ => None,
                })
            })
            .unwrap_or(Integer::I32);
        if negative && !integer.signed() {
            return Err(error(span, "negative literal requires signed integer type"));
        }
        let width = integer.bits();
        let limit = if integer.signed() {
            (1u128 << (width - 1)) - if negative { 0 } else { 1 }
        } else {
            (1u128 << width) - 1
        };
        if magnitude > limit {
            return Err(error(
                span,
                "integer literal is outside its immediately selected type",
            ));
        }
        let bits = if negative {
            (0u64).wrapping_sub(magnitude as u64)
        } else {
            magnitude as u64
        };
        // HIR literals store width-truncated bit patterns, including signed MIN.
        let bits = if width == 64 {
            bits
        } else {
            bits & ((1u64 << width) - 1)
        };
        Ok((self.types.intern(T::Int(integer)), H::Integer(bits)))
    }
}

fn integer_type(name: &str) -> Option<Integer> {
    Some(match name {
        "u8" => Integer::U8,
        "u16" => Integer::U16,
        "u32" => Integer::U32,
        "u64" => Integer::U64,
        "usize" => Integer::Usize,
        "i8" => Integer::I8,
        "i16" => Integer::I16,
        "i32" => Integer::I32,
        "i64" => Integer::I64,
        "isize" => Integer::Isize,
        _ => return None,
    })
}
fn literal_suffix(text: &str) -> Option<(Integer, &'static str)> {
    [
        "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize",
    ]
    .into_iter()
    .find_map(|name| {
        text.ends_with(&format!("_{name}"))
            .then(|| (integer_type(name).unwrap(), name))
    })
}
