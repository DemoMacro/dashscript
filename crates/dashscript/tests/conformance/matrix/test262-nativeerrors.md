# DashScript Conformance Matrix

- 26 features: **20** supported, **6** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## nativeerrors

| feature                                                                             | status       | detail / note                                                                                               |
| ----------------------------------------------------------------------------------- | ------------ | ----------------------------------------------------------------------------------------------------------- |
| test262.test.built-ins.nativeerrors.evalerror.constructor                           | 🟢 supported | _oracle: matched_                                                                                           |
| test262.test.built-ins.nativeerrors.evalerror.proto                                 | 🟡 partial   | oracle diff: line 1: ds="[Function: Error]" node="[Function: Error] { stackTraceLimit: 10 }" _oracle: diff_ |
| test262.test.built-ins.nativeerrors.evalerror.prototype.not-error-object            | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.evalerror.prototype.proto                       | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.nativeerror-tostring-message-throws-symbol      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.nativeerror-tostring-message-throws-toprimitive | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.rangeerror.constructor                          | 🟢 supported | _oracle: matched_                                                                                           |
| test262.test.built-ins.nativeerrors.rangeerror.proto                                | 🟡 partial   | oracle diff: line 1: ds="[Function: Error]" node="[Function: Error] { stackTraceLimit: 10 }" _oracle: diff_ |
| test262.test.built-ins.nativeerrors.rangeerror.prototype.not-error-object           | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.rangeerror.prototype.proto                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.referenceerror.constructor                      | 🟢 supported | _oracle: matched_                                                                                           |
| test262.test.built-ins.nativeerrors.referenceerror.proto                            | 🟡 partial   | oracle diff: line 1: ds="[Function: Error]" node="[Function: Error] { stackTraceLimit: 10 }" _oracle: diff_ |
| test262.test.built-ins.nativeerrors.referenceerror.prototype.not-error-object       | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.referenceerror.prototype.proto                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.syntaxerror.constructor                         | 🟢 supported | _oracle: matched_                                                                                           |
| test262.test.built-ins.nativeerrors.syntaxerror.proto                               | 🟡 partial   | oracle diff: line 1: ds="[Function: Error]" node="[Function: Error] { stackTraceLimit: 10 }" _oracle: diff_ |
| test262.test.built-ins.nativeerrors.syntaxerror.prototype.not-error-object          | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.syntaxerror.prototype.proto                     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.typeerror.constructor                           | 🟢 supported | _oracle: matched_                                                                                           |
| test262.test.built-ins.nativeerrors.typeerror.proto                                 | 🟡 partial   | oracle diff: line 1: ds="[Function: Error]" node="[Function: Error] { stackTraceLimit: 10 }" _oracle: diff_ |
| test262.test.built-ins.nativeerrors.typeerror.prototype.not-error-object            | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.typeerror.prototype.proto                       | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.urierror.constructor                            | 🟢 supported | _oracle: matched_                                                                                           |
| test262.test.built-ins.nativeerrors.urierror.proto                                  | 🟡 partial   | oracle diff: line 1: ds="[Function: Error]" node="[Function: Error] { stackTraceLimit: 10 }" _oracle: diff_ |
| test262.test.built-ins.nativeerrors.urierror.prototype.not-error-object             | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |
| test262.test.built-ins.nativeerrors.urierror.prototype.proto                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                                       |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
