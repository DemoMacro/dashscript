# DashScript Conformance Matrix

- 33 features: **25** supported, **4** partial, **4** unsupported, **0** untested
- correctness cases passing: 0

## proxy

| feature                                                                          | status         | detail / note                                                                                                              |
| -------------------------------------------------------------------------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------- |
| test262.test.built-ins.proxy.apply.arguments-realm                               | 🔴 unsupported | node + ds both error: exit exit code: 1: <path>oracle.ts:2 var f = $262.createRealm().global.eval( ^ Re                    |
| test262.test.built-ins.proxy.apply.null-handler                                  | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.apply.null-handler-realm                            | 🔴 unsupported | node + ds both error: exit exit code: 1: <path>oracle.ts:2 var OProxy = $262.createRealm().global.Proxy;                   |
| test262.test.built-ins.proxy.constructor                                         | 🟢 supported   | _oracle: matched_                                                                                                          |
| test262.test.built-ins.proxy.defineproperty.null-handler                         | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.defineproperty.null-handler-realm                   | 🔴 unsupported | node + ds both error: exit exit code: 1: <path>oracle.ts:2 var OProxy = $262.createRealm().global.Proxy;                   |
| test262.test.built-ins.proxy.deleteproperty.null-handler                         | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.function-prototype                                  | 🟡 partial     | oracle diff: line 1: ds="[Function: (anonymous)]" node="[Function (anonymous)] Object" _oracle: diff_                      |
| test262.test.built-ins.proxy.get.null-handler                                    | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.getownpropertydescriptor.null-handler               | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.getprototypeof.null-handler                         | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.has.null-handler                                    | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.has.null-handler-using-with                         | 🟡 partial     | node oracle error: exit exit code: 1: <path>oracle.ts:7 try { (function() { with(p.proxy) { ^^^^ (att _oracle: node-error_ |
| test262.test.built-ins.proxy.isextensible.null-handler                           | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.ownkeys.null-handler                                | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.preventextensions.null-handler                      | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.property-order                                      | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.proxy-no-prototype                                  | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.proxy-undefined-newtarget                           | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.builtin                                   | 🟡 partial     | oracle diff: line 4: ds="[Function: (anonymous)]" node="[Function (anonymous)] Object" _oracle: diff_                      |
| test262.test.built-ins.proxy.revocable.handler-is-revoked-proxy                  | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.proxy                                     | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.revocation-function-extensible            | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.revocation-function-property-order        | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.revocation-function-prototype             | 🟡 partial     | oracle diff: line 1: ds="[Function: (anonymous)]" node="[Function (anonymous)] Object" _oracle: diff_                      |
| test262.test.built-ins.proxy.revocable.revoke                                    | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.revoke-consecutive-call-returns-undefined | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.revoke-returns-undefined                  | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.target-is-revoked-function-proxy          | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.target-is-revoked-proxy                   | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.revocable.tco-fn-realm                              | 🔴 unsupported | node + ds both error: exit exit code: 1: <path>oracle.ts:2 var other = $262.createRealm(); ^ Refere                        |
| test262.test.built-ins.proxy.set.null-handler                                    | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |
| test262.test.built-ins.proxy.setprototypeof.null-handler                         | 🟢 supported   | via rquickjs engine _oracle: matched_                                                                                      |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
