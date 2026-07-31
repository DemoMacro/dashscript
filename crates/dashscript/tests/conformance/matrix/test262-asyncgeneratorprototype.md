# DashScript Conformance Matrix

- 31 features: **3** supported, **1** partial, **27** unsupported, **0** untested
- correctness cases passing: 0

## asyncgeneratorprototype

| feature                                                                                              | status         | detail / note                                                   |
| ---------------------------------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------- |
| test262.test.built-ins.asyncgeneratorprototype.next.iterator-result-prototype                        | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.next.request-queue-await-order                        | 🟡 partial     | Test262Error: Test262Error: asyncTest called without async flag |
| test262.test.built-ins.asyncgeneratorprototype.next.request-queue-order                              | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.next.request-queue-order-state-executing              | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.next.request-queue-promise-resolve-order              | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.next.return-promise                                   | 🟢 supported   | via rquickjs engine                                             |
| test262.test.built-ins.asyncgeneratorprototype.return.iterator-result-prototype                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.request-queue-order-state-executing            | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-promise                                 | 🟢 supported   | via rquickjs engine                                             |
| test262.test.built-ins.asyncgeneratorprototype.return.return-state-completed                         | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-state-completed-broken-promise          | 🟢 supported   | via rquickjs engine                                             |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedstart                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedstart-broken-promise           | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedstart-promise                  | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedyield                          | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedyield-broken-promise-try-catch | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedyield-promise                  | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedyield-try-finally              | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedyield-try-finally-return       | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.return.return-suspendedyield-try-finally-throw        | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.request-queue-order-state-executing             | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.return-rejected-promise                         | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-state-completed                           | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedstart                            | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedstart-promise                    | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedyield                            | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedyield-promise                    | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedyield-try-catch                  | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedyield-try-finally                | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedyield-try-finally-return         | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |
| test262.test.built-ins.asyncgeneratorprototype.throw.throw-suspendedyield-try-finally-throw          | 🔴 unsupported | engine lacks built-in: ReferenceError: $DONE is not defined     |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
