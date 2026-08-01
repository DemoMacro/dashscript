# DashScript Conformance Matrix

- 38 features: **38** supported, **0** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## asyncfromsynciteratorprototype

| feature                                                                                                   | status       | detail / note       |
| --------------------------------------------------------------------------------------------------------- | ------------ | ------------------- |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.absent-value-not-passed                        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.for-await-iterator-next-rejected-promise-close | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.for-await-next-rejected-promise-close          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-poisoned-done                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-poisoned-value                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-poisoned-wrapper               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-prototype                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-rejected                       | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-unwrap-promise                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.next-result-poisoned-wrapper                   | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.return-promise                                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.yield-iterator-next-rejected-promise-close     | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.yield-next-rejected-promise-close              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.absent-value-not-passed                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result-poisoned-done                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result-poisoned-value               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result-unwrap-promise               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.poisoned-get-return                          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.poisoned-return                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.result-object-error                          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.return-null                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.return-undefined                             | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-poisoned-done                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-poisoned-value                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-rejected-promise-close        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-unwrap-promise                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.poisoned-get-throw                            | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.poisoned-throw                                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.result-object-error                           | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-null                                    | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-result-poisoned-wrapper                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-get-return-undefined          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-poisoned-return               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-return-not-object             | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-return-object                 | 🟢 supported | via rquickjs engine |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
