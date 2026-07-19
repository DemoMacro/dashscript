# DashScript Conformance Matrix

- 27 features: **25** supported, **2** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## generatorprototype

| feature                                                                                                      | status       | detail / note                                                                                      |
| ------------------------------------------------------------------------------------------------------------ | ------------ | -------------------------------------------------------------------------------------------------- |
| test262.test.built-ins.generatorprototype.next.consecutive-yields                                            | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.context-method-invocation                                     | 🟡 partial   | oracle diff: line 1: ds="{ g: [Function: g] }" node="{ g: [GeneratorFunction: g] }" _oracle: diff_ |
| test262.test.built-ins.generatorprototype.next.from-state-executing                                          | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.lone-return                                                   | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.lone-yield                                                    | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.no-control-flow                                               | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.result-prototype                                              | 🟡 partial   | oracle diff: line 3: ds="{}" node="[Object: null prototype] {}" _oracle: diff_                     |
| test262.test.built-ins.generatorprototype.next.return-yield-expr                                             | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.this-val-not-generator                                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.next.this-val-not-object                                           | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.from-state-completed                                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.from-state-executing                                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.from-state-suspended-start                                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.this-val-not-generator                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.this-val-not-object                                         | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-catch-before-try                                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-catch-within-try                                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-before-try                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-following-finally                               | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-nested-try-catch-within-inner-try               | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-nested-try-catch-within-outer-try-before-nested | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-set-property-within-try                         | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-within-finally                                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.return.try-finally-within-try                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.throw.from-state-executing                                         | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.throw.this-val-not-generator                                       | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |
| test262.test.built-ins.generatorprototype.throw.this-val-not-object                                          | 🟢 supported | via rquickjs engine _oracle: matched_                                                              |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
