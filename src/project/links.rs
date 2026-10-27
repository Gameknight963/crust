use super::*;

impl Project {
    pub fn output_base(&self, target: &Target) -> PathBuf {
        return self
            .directory
            .join("target/crust")
            .join(&target.name)
            .join(&target.name);
    }

    pub fn ordered_targets(&self, selected: &[&Target]) -> Result<Vec<&Target>, String> {
        let names: HashMap<_, _> = self
            .targets
            .iter()
            .enumerate()
            .map(|(index, target)| (target.name.as_str(), index))
            .collect();
        let mut dependencies = Vec::new();
        for target in &self.targets {
            let mut edges = Vec::new();
            let mut seen = HashSet::new();
            for name in &target.native.link_targets {
                let Some(index) = names.get(name.as_str()).copied() else {
                    return Err(format!(
                        "{}: target '{}' links unknown target '{name}'",
                        self.manifest.display(),
                        target.name
                    ));
                };
                if !seen.insert(index) {
                    return Err(format!(
                        "{}: target '{}' repeats linked target '{name}'",
                        self.manifest.display(),
                        target.name
                    ));
                }
                if self.targets[index].kind == TargetKind::Executable {
                    return Err(format!(
                        "{}: target '{}' links '{name}', which must be a static-library or shared-library target",
                        self.manifest.display(),
                        target.name
                    ));
                }
                edges.push(index);
            }
            dependencies.push(edges);
        }
        let mut states = vec![0; self.targets.len()];
        let mut order = Vec::new();
        for target in selected {
            let root = names
                .get(target.name.as_str())
                .copied()
                .ok_or_else(|| format!("unknown target '{}'", target.name))?;
            if states[root] == 2 {
                continue;
            }
            let mut stack = vec![(root, 0)];
            states[root] = 1;
            while let Some((index, next)) = stack.last_mut() {
                if let Some(dependency) = dependencies[*index].get(*next).copied() {
                    *next += 1;
                    match states[dependency] {
                        0 => {
                            states[dependency] = 1;
                            stack.push((dependency, 0));
                        }
                        1 => {
                            let start = stack
                                .iter()
                                .position(|(index, _)| *index == dependency)
                                .unwrap();
                            let cycle = stack[start..]
                                .iter()
                                .map(|(index, _)| self.targets[*index].name.as_str())
                                .chain(std::iter::once(self.targets[dependency].name.as_str()))
                                .collect::<Vec<_>>()
                                .join(" -> ");
                            return Err(format!(
                                "{}: native target link cycle: {cycle}",
                                self.manifest.display()
                            ));
                        }
                        _ => {}
                    }
                } else {
                    states[*index] = 2;
                    order.push(&self.targets[*index]);
                    stack.pop();
                }
            }
        }
        return Ok(order);
    }

    pub fn resolved_native(&self, target: &Target) -> Result<crate::native::Options, String> {
        let mut native = target.native.clone();
        native.link_targets.clear();
        if target.kind != TargetKind::StaticLibrary {
            for dependency in self.ordered_targets(&[target])?.into_iter().rev() {
                if dependency.name != target.name {
                    native
                        .link_inputs
                        .push(dependency.kind.link_path(&self.output_base(dependency))?);
                }
            }
        }
        native.runtime_dirs = self.runtime_paths(target)?;
        native.runtime_libraries = self
            .ordered_targets(&[target])?
            .into_iter()
            .filter(|dependency| {
                dependency.kind == TargetKind::SharedLibrary && dependency.name != target.name
            })
            .map(|dependency| dependency.kind.artifact_path(&self.output_base(dependency)))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(native);
    }

    pub fn runtime_paths(&self, target: &Target) -> Result<Vec<PathBuf>, String> {
        return Ok(self
            .ordered_targets(&[target])?
            .into_iter()
            .filter(|dependency| {
                dependency.kind == TargetKind::SharedLibrary && dependency.name != target.name
            })
            .map(|dependency| self.output_base(dependency).parent().unwrap().to_owned())
            .collect());
    }
}
