use super::*;

impl Project {
    pub fn output_base(&self, target: &Target) -> PathBuf {
        let mut output = self.directory.join("target/crust");
        if target.name.contains(':') {
            output.push("dependencies");
        }
        for segment in target.name.split(':') {
            output.push(segment);
        }
        output.push(target.name.rsplit(':').next().unwrap());
        return output;
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
        let dependencies = self.ordered_targets(&[target])?;
        for dependency in &dependencies {
            for sanitizer in &dependency.native.sanitizers {
                if !native.sanitizers.contains(sanitizer) {
                    native.sanitizers.push(*sanitizer);
                }
            }
        }
        for dependency in dependencies.into_iter().rev() {
            if dependency.name != target.name {
                for directory in &dependency.native.library_dirs {
                    if !native.library_dirs.contains(directory) {
                        native.library_dirs.push(directory.clone());
                    }
                }
                // Keep library order and repeated names needed by static linkers.
                native
                    .libraries
                    .extend(dependency.native.libraries.iter().cloned());
                if target.kind != TargetKind::StaticLibrary {
                    native
                        .link_inputs
                        .push(dependency.kind.link_path(&self.output_base(dependency))?);
                }
            }
        }
        native.runtime_libraries = self.runtime_libraries(target)?;
        native.runtime_dirs = runtime_directories(&native.runtime_libraries);
        return Ok(native);
    }

    pub fn runtime_paths(&self, target: &Target) -> Result<Vec<PathBuf>, String> {
        return Ok(runtime_directories(&self.runtime_libraries(target)?));
    }

    pub fn runtime_libraries(&self, target: &Target) -> Result<Vec<PathBuf>, String> {
        let mut libraries = Vec::new();
        let mut seen = HashSet::new();
        for dependency in self.ordered_targets(&[target])? {
            let mut files = dependency.native.runtime_libraries.clone();
            if dependency.kind == TargetKind::SharedLibrary && dependency.name != target.name {
                files.push(
                    dependency
                        .kind
                        .artifact_path(&self.output_base(dependency))?,
                );
            }
            for file in files {
                let file = crate::source::identity(&file).map_err(|error| error.to_string())?;
                if seen.insert(file.clone()) {
                    libraries.push(file);
                }
            }
        }
        return Ok(libraries);
    }
}

pub(super) fn runtime_directories(libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    return libraries
        .iter()
        .filter_map(|library| library.parent())
        .filter(|directory| seen.insert(directory.to_path_buf()))
        .map(Path::to_path_buf)
        .collect();
}
