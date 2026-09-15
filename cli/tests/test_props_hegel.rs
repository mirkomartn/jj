// Copyright 2026 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs::OpenOptions;
use std::fs::create_dir_all;
use std::io::Write as _;

use hegel::TestCase;
use hegel::generators as gs;
use hegel::generators::Generator as _;
use itertools::Itertools as _;

use crate::common::TestEnvironment;

mod revsets;

fn draw_file_contents(tc: &TestCase) -> Vec<u8> {
    tc.draw(hegel::one_of!(
        // Empty files represent a significant edge case, so we want to increase the likelihood of
        // empty file contents in subsequent transitions.
        gs::just(Vec::new()),
        // [0] is the simplest "binary" file and it's included here to increase the likelihood of
        // identical binary file contents in subsequent transition.
        gs::just(vec![0_u8]),
        // Diffing is line-oriented, so try to generate files with relatively
        // many newlines.
        gs::vecs(hegel::one_of!(
            gs::just('\n'),
            gs::characters()
                .min_codepoint('a' as u32)
                .max_codepoint('z' as u32),
            gs::characters().exclude_categories(&["Cc", "Cf", "Cs", "Co", "Cn"]),
        ))
        .map(|chars| chars.into_iter().collect::<String>().into_bytes()),
        // Arbitrary binary contents, not limited to valid UTF-8.
        gs::binary().max_size(31),
    ))
}

fn draw_path_component(tc: &TestCase) -> String {
    // HACK: Forbidding `.` here to avoid `.`/`..` in the path components, which
    // causes downstream errors.
    tc.draw(hegel::one_of!(
        gs::just("a".to_owned()),
        gs::just("b".to_owned()),
        gs::just("c".to_owned()),
        gs::just("d".to_owned()),
        gs::text()
            .min_size(1)
            .exclude_categories(&["Cc", "Cf", "Cs", "Co", "Cn"])
            .exclude_characters("/."),
    ))
}

struct JjCli {
    test_env: TestEnvironment,
}

#[hegel::state_machine]
impl JjCli {
    fn new() -> Self {
        let test_env = TestEnvironment::default();
        test_env.run_jj_in(".", ["git", "init", "base"]).success();
        test_env
            .run_jj_in(".", ["git", "init", "fallible"])
            .success();

        Self { test_env }
    }

    #[inline]
    fn change_ids(&self, revset: &str, repo: &str, at_op: Option<&str>) -> Vec<String> {
        let work_dir = self.test_env.work_dir(repo);
        let revs = work_dir
            .run_jj(&[
                "log",
                "-r",
                revset,
                "-T",
                "change_id ++ '|'",
                "-G",
                "--ignore-working-copy",
                "--at-operation",
                at_op.unwrap_or("@"),
            ])
            .stdout;

        revs.raw()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(Into::into)
            .collect()
    }

    #[allow(unused)]
    #[inline]
    fn commit_ids(&self, revset: &str, repo: &str) -> Vec<String> {
        let work_dir = self.test_env.work_dir(repo);
        let revs = work_dir
            .run_jj(&[
                "log",
                "-r",
                revset,
                "-T",
                "commit_id ++ '|'",
                "-G",
                "--ignore-working-copy",
            ])
            .stdout;

        revs.raw()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(Into::into)
            .collect()
    }

    #[inline]
    fn operations(&self, repo: &str) -> Vec<String> {
        let work_dir = self.test_env.work_dir(repo);
        let ops = work_dir
            .run_jj(&[
                "op",
                "log",
                "-T",
                "id ++ '|'",
                "-G",
                "--ignore-working-copy",
            ])
            .stdout;

        ops.raw()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(Into::into)
            .collect()
    }

    #[inline]
    fn draw_revset(&self, tc: TestCase, repo: &str) -> String {
        let ops = self.operations(repo);
        revsets::draw_revset(&tc, 4, &ops)
    }

    #[invariant]
    fn log_never_panics(&self, tc: TestCase) {
        let revset = self.draw_revset(tc.clone(), "base");
        let work_dir = self.test_env.work_dir("base");
        let succ = work_dir.run_jj(["log", "-r", &revset, "--no-integrate-operation"]);

        // Panics return 101
        if succ.status.code().unwrap_or(0) == 101 {
            let oplog = work_dir.run_jj(["op", "log"]).stdout;
            let graph = work_dir.run_jj(["log", "-r", "all()"]).stdout;

            eprintln!("{graph}\n\n{oplog}");
            eprintln!("base: log panicked with {revset}");
            panic!();
        }

        let revset = self.draw_revset(tc, "fallible");
        let work_dir = self.test_env.work_dir("fallible");
        let succ = work_dir.run_jj(["log", "-r", &revset, "--no-integrate-operation"]);

        // Panics return 101
        if succ.status.code().unwrap_or(0) == 101 {
            let oplog = work_dir.run_jj(["op", "log"]).stdout;
            let graph = work_dir.run_jj(["log", "-r", "all()"]).stdout;

            eprintln!("{graph}\n\n{oplog}");
            eprintln!("fallible log panicked with {revset}");
            panic!();
        }
    }

    #[rule]
    fn write_file(&mut self, tc: TestCase) {
        let path = draw_path_component(&tc);
        let file = draw_file_contents(&tc);

        let append = tc.draw(gs::booleans());

        let base = self.test_env.work_dir("base");
        {
            let path = base.root().join(&path);

            create_dir_all(path.parent().unwrap()).unwrap();

            OpenOptions::new()
                .write(true)
                .create(true)
                .append(append)
                .open(path)
                .unwrap()
                .write_all(file.as_slice())
                .unwrap();
        }

        let fallible = self.test_env.work_dir("fallible");
        let path = fallible.root().join(path);

        create_dir_all(path.parent().unwrap()).unwrap();

        OpenOptions::new()
            .write(true)
            .create(true)
            .append(append)
            .open(path)
            .unwrap()
            .write_all(file.as_slice())
            .unwrap();
    }

    #[rule]
    fn jj_cli(&mut self, tc: TestCase) {
        let ignore_immutable = tc.draw(gs::booleans());
        // Current limit: can't work with --at-operation -- there's a bug in the upstream.
        let at_operation: Option<String> = None; // tc.draw(gs::optional(gs::sampled_from(self.operations("base"))));

        let revisions = if ignore_immutable {
            self.change_ids(
                "all() & ~root() & ~divergent()",
                "base",
                at_operation.as_deref(),
            )
        } else {
            self.change_ids("mutable() & ~divergent()", "base", at_operation.as_deref())
        };

        let mut actions: Vec<_> = vec![];

        if !revisions.is_empty() {
            actions.extend_from_slice(&[Self::jj_edit, Self::jj_absorb]);
        }

        if revisions.len() > 1 {
            actions.extend_from_slice(&[
                Self::jj_duplicate,
                Self::jj_duplicate,
                Self::jj_parallelize,
                Self::jj_rebase,
                Self::jj_restore,
                Self::jj_squash,
            ]);
        }

        actions.extend_from_slice(&[
            Self::jj_new,
            Self::jj_new,
            Self::jj_new,
            Self::jj_new,
            Self::jj_new,
            Self::jj_new,
        ]);

        let action = tc.draw(gs::sampled_from(&actions));

        let fail_switches = tc.draw(
            gs::vecs(gs::sampled_from(&[
                ("JJ_PROPS_MONKEY_BACKEND_ERROR", "1"),
                ("JJ_PROPS_MONKEY_INDEX_STORE_ERROR", "1"),
                ("JJ_PROPS_MONKEY_INDEX_ERROR", "1"),
                ("JJ_PROPS_MONKEY_OP_STORE_ERROR", "1"),
            ]))
            .unique(true)
            .min_size(1),
        );

        let args = action(
            self,
            tc.clone(),
            ignore_immutable,
            at_operation.as_deref(),
            "base",
        );

        let base = self.test_env.work_dir("base");

        // LIMIT: Always runs from the repo-relative root directory.
        base.run_jj_with(|cmd| cmd.args(&args)).success();

        let succ = base.run_jj(["workspace", "update-stale"]);
        if !succ.status.success() {
            eprintln!("base: `jj workspace update-stale` failed.");

            eprintln!("{}", succ.stderr);
            eprintln!("{}", succ.stdout);
            succ.success();
        }

        let fail_seed = tc.draw(gs::integers::<u64>()).to_string();
        // TODO: it should be the same operation, if anything
        // let at_operation = tc.draw(gs::optional(gs::sampled_from(self.operations("fallible"))));
        let at_operation: Option<String> = None;

        let args = action(
            self,
            tc.clone(),
            ignore_immutable,
            at_operation.as_deref(),
            "fallible",
        );
        let fallible = self.test_env.work_dir("fallible");

        let succ = fallible.run_jj_with(|cmd| {
            cmd.args(&args)
                .envs(fail_switches)
                .env("JJ_PROPS_MONKEY_SEED", fail_seed)
        });

        if !succ.status.success() {
            let succ = fallible.run_jj(["workspace", "update-stale"]);
            if !succ.status.success() {
                eprintln!("fallible: `jj workspace update-stale` failed.");

                eprintln!("{}", succ.stderr);
                eprintln!("{}", succ.stdout);
                succ.success();
            }

            let succ = fallible.run_jj_with(|cmd| cmd.args(&args));
            if !succ.status.success() {
                eprintln!("fallible: `jj {}` failed", args.join(" "));

                eprintln!("{}", succ.stderr);
                eprintln!("{}", succ.stdout);
                succ.success();
            }
        }
    }

    #[allow(unused)]
    fn jj_abandon(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids(
                "all() & ~merges()- & ~root()+ & ~root()",
                repo,
                at_operation,
            )
        } else {
            self.change_ids("mutable() & ~merges()- & ~root()+", repo, at_operation)
        };

        tc.assume(!revisions.is_empty());

        let revs = tc
            .draw(gs::vecs(gs::sampled_from(&revisions)).min_size(1))
            .iter()
            .map(|s| format!("change_id({s})")) // Needed for divergent changes, so that all get selected.
            .join("|");
        let restore_descendants = tc.draw(gs::booleans());

        let args = {
            let mut ret = vec![
                "abandon",
                "-r",
                revs.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            if restore_descendants {
                ret.push("--restore-descendants");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_absorb(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids(
                "all() & ~root() & ~root()+ & ~divergent()",
                repo,
                at_operation,
            )
        } else {
            self.change_ids("mutable() & ~root()+ & ~divergent()", repo, at_operation)
        };

        tc.assume(!revisions.is_empty());

        let rev = tc.draw(gs::sampled_from(&revisions));

        let args = {
            let mut ret = vec![
                "absorb",
                "-f",
                rev.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_duplicate(
        &mut self,
        tc: TestCase,
        _ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = self.change_ids("all() & ~root()", repo, at_operation);
        tc.assume(revisions.len() > 1);

        let duplicates = tc
            .draw(
                gs::vecs(gs::sampled_from(&revisions))
                    .max_size(revisions.len() - 1)
                    .min_size(1),
            )
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("|");
        let parents = tc
            .draw(
                gs::vecs(gs::sampled_from(
                    revisions
                        .iter()
                        .filter(|x| !duplicates.contains(*x))
                        .collect::<Vec<&String>>(),
                ))
                .min_size(1),
            )
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("|");

        let args = [
            "duplicate",
            "-r",
            duplicates.as_str(),
            "-o",
            parents.as_str(),
            "--at-operation",
            at_operation.unwrap_or("@"),
        ];

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_edit(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids("all() & ~root() & ~divergent()", repo, at_operation)
        } else {
            self.change_ids("mutable() & ~divergent()", repo, at_operation)
        };

        tc.assume(!revisions.is_empty());

        let rev = tc.draw(gs::sampled_from(&revisions));

        let args = {
            let mut ret = vec![
                "edit",
                "-r",
                rev.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_new(
        &mut self,
        tc: TestCase,
        _ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = self.change_ids("all()", repo, at_operation);
        tc.assume(!revisions.is_empty());

        let revset = tc.draw(
            gs::vecs(gs::sampled_from(&revisions))
                .min_size(1)
                .unique(true),
        );

        // Cannot create merge commit with root().
        let revset = if revset.len() > 1 {
            let revset = revset
                .into_iter()
                .map(|s| format!("change_id({s})"))
                .join("|");
            format!("({revset}) & ~root()")
        } else {
            format!("change_id({})", revset[0])
        };

        let args = [
            "new",
            "-r",
            revset.as_str(),
            "--at-operation",
            at_operation.unwrap_or("@"),
        ];

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_parallelize(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids("all() & ~root() & ~root()+", repo, at_operation)
        } else {
            self.change_ids("mutable() & ~root()+", repo, at_operation)
        };
        tc.assume(!revisions.is_empty());

        let revset = tc
            .draw(gs::vecs(gs::sampled_from(&revisions)).min_size(2))
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("|");

        let args = {
            let mut ret = vec![
                "parallelize",
                "-r",
                revset.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_rebase(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids("all() & ~root() & ~root()+", repo, at_operation)
        } else {
            self.change_ids("mutable() & ~root()+", repo, at_operation)
        };
        tc.assume(revisions.len() > 1);

        let commits = tc
            .draw(
                gs::vecs(gs::sampled_from(&revisions))
                    .max_size(revisions.len() - 1)
                    .min_size(1),
            )
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("|");

        let parents = tc
            .draw(
                gs::vecs(gs::sampled_from(
                    revisions
                        .iter()
                        .filter(|x| !commits.contains(*x))
                        .collect::<Vec<&String>>(),
                ))
                .min_size(1),
            )
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("|");

        let args = {
            let mut ret = vec![
                "rebase",
                "-r",
                commits.as_str(),
                "-o",
                parents.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_restore(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids("all() & ~root() & ~divergent()", repo, at_operation)
        } else {
            self.change_ids("mutable() & ~divergent()", repo, at_operation)
        };
        tc.assume(!revisions.is_empty());

        let all_no_root = self.change_ids("all() & ~root() & ~divergent()", repo, at_operation);
        tc.assume(!all_no_root.is_empty());

        let into = tc.draw(gs::sampled_from(&revisions));
        let from = tc.draw(gs::sampled_from(
            all_no_root
                .iter()
                .filter(|x| x.as_str() != into)
                .collect::<Vec<&String>>(),
        ));

        let args = {
            let mut ret = vec![
                "restore",
                "-f",
                from,
                "-t",
                into.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    #[allow(unused)]
    fn jj_revert(
        &mut self,
        tc: TestCase,
        _ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let all = self.change_ids("all()", repo, at_operation);
        let all_no_root = self.change_ids("all() & ~root()", repo, at_operation);

        tc.assume(!all.is_empty());
        tc.assume(!all_no_root.is_empty());

        let onto = tc
            .draw(gs::vecs(gs::sampled_from(&all)).min_size(1))
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("|");
        let revs = tc
            .draw(gs::vecs(gs::sampled_from(&all_no_root)).min_size(1))
            .into_iter()
            .map(|s| format!("change_id({s})"))
            .join("| ");

        let args = [
            "revert",
            "-r",
            revs.as_str(),
            "-o",
            onto.as_str(),
            "--at-operation",
            at_operation.unwrap_or("@"),
        ];

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }

    fn jj_squash(
        &mut self,
        tc: TestCase,
        ignore_immutable: bool,
        at_operation: Option<&str>,
        repo: &str,
    ) -> Vec<String> {
        let revisions = if ignore_immutable {
            self.change_ids(
                "all() & ~root()+ & root() & ~divergent()",
                repo,
                at_operation,
            )
        } else {
            self.change_ids("mutable() & ~root()+ & ~divergent()", repo, at_operation)
        };
        tc.assume(revisions.len() > 2);

        let from = tc
            .draw(
                gs::vecs(gs::sampled_from(&revisions))
                    .max_size(revisions.len() - 1)
                    .min_size(1),
            )
            .join("|");
        let to = tc.draw(gs::sampled_from(
            revisions
                .iter()
                .filter(|x| !from.contains(*x))
                .collect::<Vec<&String>>(),
        ));

        let args = {
            let mut ret = vec![
                "squash",
                "-f",
                from.as_str(),
                "-t",
                to.as_str(),
                "--at-operation",
                at_operation.unwrap_or("@"),
            ];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        args.into_iter().map(Into::into).collect::<Vec<String>>()
    }
}

#[hegel::test(hegel::Settings::new().print_blob(true).suppress_health_check
    ([hegel::HealthCheck::TooSlow]).verbosity(hegel::Verbosity::Quiet))]
fn test_aufhebung(tc: TestCase) {
    let jjcli = JjCli::new();
    hegel::stateful::run(jjcli, tc);
}
