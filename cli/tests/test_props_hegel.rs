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

use hegel::TestCase;
use hegel::generators as gs;
use itertools::Itertools as _;

use crate::common::TestEnvironment;

struct JjCli {
    test_env: TestEnvironment,
}

#[hegel::state_machine]
impl JjCli {
    fn new() -> Self {
        let test_env = TestEnvironment::default();
        test_env.run_jj_in(".", ["git", "init", "repo"]).success();

        Self { test_env }
    }

    fn revisions(&mut self, revset: &str) -> Vec<String> {
        let work_dir = self.test_env.work_dir("repo");
        let all_no_root_revs = work_dir
            .run_jj(&[
                "log",
                "-r",
                revset,
                "-T",
                "change_id ++ '|'",
                "-G",
                "--ignore-working-copy",
            ])
            .stdout;

        all_no_root_revs
            .raw()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(Into::into)
            .collect()
    }

    #[invariant]
    fn check_invariants(&self, _: TestCase) {
        let work_dir = self.test_env.work_dir("repo");
        let succ = work_dir.run_jj(["log", "-r", "at_operation(@-, all())"]);

        if !succ.status.success() {
            let oplog = work_dir.run_jj(["op", "log"]).stderr;
            let graph = work_dir.run_jj(["log", "-r", "all()"]).stderr;

            eprintln!("{graph}\n\n{oplog}");
            succ.success();
        }
    }

    #[rule]
    fn jj_cli(&mut self, tc: TestCase) {
        let ignore_immutable = tc.draw(gs::booleans());

        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
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

        action(self, tc, false);
    }

    #[allow(unused)]
    fn jj_abandon(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };
        let work_dir = self.test_env.work_dir("repo");

        let revs = tc
            .draw(gs::vecs(gs::sampled_from(&revisions)).min_size(1))
            .join("|");
        let restore_descendants = tc.draw(gs::booleans());

        let args = {
            let mut ret = vec!["abandon", "-r", revs.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            if restore_descendants {
                ret.push("--restore-descendants");
            }

            ret
        };

        work_dir.run_jj(&args);
    }

    fn jj_absorb(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };

        let work_dir = self.test_env.work_dir("repo");

        let rev = tc.draw(gs::sampled_from(&revisions));

        let args = {
            let mut ret = vec!["absorb", "-f", rev.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        work_dir.run_jj(&args);
    }

    fn jj_duplicate(&mut self, tc: TestCase, _ignore_immutable: bool) {
        let revisions = self.revisions("all() & ~root()");
        let work_dir = self.test_env.work_dir("repo");

        let duplicates = tc.draw(
            gs::vecs(gs::sampled_from(&revisions))
                .max_size(revisions.len() - 1)
                .min_size(1),
        );
        let parents = tc.draw(
            gs::vecs(gs::sampled_from(
                revisions
                    .iter()
                    .filter(|x| !duplicates.contains(x))
                    .collect::<Vec<&String>>(),
            ))
            .min_size(1),
        );

        work_dir.run_jj(&[
            "duplicate",
            "-r",
            duplicates.join("|").as_str(),
            "-o",
            parents.into_iter().join("|").as_str(),
        ]);
    }

    fn jj_edit(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };
        let work_dir = self.test_env.work_dir("repo");

        let rev = tc.draw(gs::sampled_from(&revisions));

        let args = {
            let mut ret = vec!["edit", "-r", rev.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        work_dir.run_jj(&args);
    }

    fn jj_new(&mut self, tc: TestCase, _ignore_immutable: bool) {
        let revisions = self.revisions("all() & ~root()"); // cannot create merge commit with root()
        let work_dir = self.test_env.work_dir("repo");

        let revset = tc.draw(gs::vecs(gs::sampled_from(&revisions)).min_size(1));

        let revset = if revset.len() > 1 {
            let revset = revset.join("|");
            format!("({revset}) & ~root()")
        } else {
            format!("change_id({})", revset[0])
        };

        work_dir.run_jj(&["new", "-r", revset.as_str()]);
    }

    fn jj_parallelize(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };
        let work_dir = self.test_env.work_dir("repo");

        let revset = tc
            .draw(gs::vecs(gs::sampled_from(&revisions)).min_size(2))
            .join("|");

        let args = {
            let mut ret = vec!["parallelize", "-r", revset.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        work_dir.run_jj(&args);
    }

    fn jj_rebase(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };
        let work_dir = self.test_env.work_dir("repo");

        let commits = tc
            .draw(
                gs::vecs(gs::sampled_from(&revisions))
                    .max_size(revisions.len() - 1)
                    .min_size(1),
            )
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
            .join("|");

        let args = {
            let mut ret = vec!["rebase", "-r", commits.as_str(), "-o", parents.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        work_dir.run_jj(&args);
    }

    fn jj_restore(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };
        let all_no_root = self.revisions("all() & ~root()");

        let work_dir = self.test_env.work_dir("repo");

        let into = tc.draw(gs::sampled_from(&revisions));
        let from = tc.draw(gs::sampled_from(
            all_no_root
                .iter()
                .filter(|x| x.as_str() != into)
                .collect::<Vec<&String>>(),
        ));

        let args = {
            let mut ret = vec!["restore", "-f", from, "t", into.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        work_dir.run_jj(&args);
    }

    #[allow(unused)]
    fn jj_revert(&mut self, tc: TestCase, _ignore_immutable: bool) {
        let all = self.revisions("all()");
        let all_no_root = self.revisions("all() & ~root()");

        let work_dir = self.test_env.work_dir("repo");

        let onto = tc.draw(gs::vecs(gs::sampled_from(&all)).min_size(1));
        let revs = tc.draw(gs::vecs(gs::sampled_from(&all_no_root)).min_size(1));

        work_dir.run_jj(&[
            "revert",
            "-r",
            revs.join("|").as_str(),
            "-o",
            onto.join("|").as_str(),
        ]);
    }

    fn jj_squash(&mut self, tc: TestCase, ignore_immutable: bool) {
        let revisions = if ignore_immutable {
            self.revisions("all() & ~root()")
        } else {
            self.revisions("mutable()")
        };
        let work_dir = self.test_env.work_dir("repo");

        let from = tc
            .draw(
                gs::vecs(gs::sampled_from(&revisions))
                    .max_size(revisions.len() - 1)
                    .min_size(1),
            )
            .join("|");
        let to = tc
            .draw(
                gs::vecs(gs::sampled_from(
                    revisions
                        .iter()
                        .filter(|x| !from.contains(*x))
                        .collect::<Vec<&String>>(),
                ))
                .min_size(1),
            )
            .into_iter()
            .join("|");

        let args = {
            let mut ret = vec!["squash", "-f", from.as_str(), "-t", to.as_str()];

            if ignore_immutable {
                ret.push("--ignore-immutable");
            }

            ret
        };

        work_dir.run_jj(&args);
    }
}

#[hegel::test(hegel::Settings::new().print_blob(true).suppress_health_check
    ([hegel::HealthCheck::TooSlow]).verbosity(hegel::Verbosity::Quiet))]
fn test_aufhebung(tc: TestCase) {
    let jjcli = JjCli::new();
    hegel::stateful::run(jjcli, tc);
}
