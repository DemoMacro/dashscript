# DashScript Conformance Matrix

- 38 features: **1** supported, **17** partial, **20** unsupported, **0** untested
- correctness cases passing: 0

## asyncfromsynciteratorprototype

| feature                                                                                                   | status         | detail / note                                                   |
| --------------------------------------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------- |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.absent-value-not-passed                        | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.for-await-iterator-next-rejected-promise-close | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.for-await-next-rejected-promise-close          | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-poisoned-done                  | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-poisoned-value                 | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-poisoned-wrapper               | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-prototype                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-rejected                       | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.iterator-result-unwrap-promise                 | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.next-result-poisoned-wrapper                   | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.return-promise                                 | 🟢 supported   | via rquickjs engine                                             |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.yield-iterator-next-rejected-promise-close     | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.next.yield-next-rejected-promise-close              | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.absent-value-not-passed                      | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result                              | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result-poisoned-done                | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result-poisoned-value               | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.iterator-result-unwrap-promise               | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.poisoned-get-return                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.poisoned-return                              | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.result-object-error                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.return-null                                  | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.return.return-undefined                             | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result                               | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-poisoned-done                 | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-poisoned-value                | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-rejected-promise-close        | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.iterator-result-unwrap-promise                | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.poisoned-get-throw                            | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.poisoned-throw                                | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.result-object-error                           | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-null                                    | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-result-poisoned-wrapper                 | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined                               | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-get-return-undefined          | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-poisoned-return               | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-return-not-object             | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncfromsynciteratorprototype.throw.throw-undefined-return-object                 | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
