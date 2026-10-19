// @lfy def/mcp/read.lfy:problems
//! Every fn of this unit that carries `tool`, as the server lists it, and how a call
//! reaches it.

use super::*;

/// The fns of this module that carry `tool`, in the order the definition declares them; no
/// resource and no prompt is listed beside them.
///
/// One [`Registered`] per fn carries the name the server lists it by, the fn's definition
/// as its description, and one [`ToolArgument`] per parameter that is not the session. A
/// call reaches the fn through [`call_tool`], which checks the arguments against those
/// parameters before the fn is called, answers an argument that is missing, unknown, or of
/// the wrong type with an error naming it and without calling the fn, and turns what the fn
/// returns into the call's result — a failure into an error carrying it, so the server goes
/// on.
///
/// Which of them a session is offered is [`offered`]; a session lists and calls only those.
pub fn tools() -> Vec<Registered> {
    vec![
        // @lfy def/mcp/read.lfy:problems#problems:tool:47dafd874509841945f72721454f0bde82f7cb760967d4104944c32a0b08e4f6
        // @lfy def/mcp/read.lfy:problems#problems:tool:8697079e758527441df828916c31420593b865e8089a7dc5367a58f9ea2131d8
        // @lfy def/mcp/read.lfy:problems#problems:tool:e3326a06fa53e9623634da620a9bac4aa2a1d0c7e0ea9f0d1143f0f4443b8ba5
        // @lfy def/mcp/read.lfy:problems#problems:tool:97369ca270f9c5503c620a5a45c2e5bda573d9d44900f052d6aa2f6fab3ccd0c
        Registered::new(
            "problems",
            "The problems of the program, or of one file: what the compiler, an editor, or a check would report",
            // @lfy def/mcp/read.lfy:problems#problems:tool:54f7dd8fa92710b384c60ba1163c17852fbfb0a80ca36261f57990307bf95f7e
            vec![
                // @lfy def/mcp/read.lfy:problems#problems:tool:18f70fcb2a52141d025ba8a3d8956133691c72b34fdf58d3db5db6df54e1f6fc
                // @lfy def/mcp/read.lfy:problems#problems:tool:0d47cd7405248873ce91f19150020622e2bbc3156320aa272d9325d8ab7ad9b4
                // @lfy def/mcp/read.lfy:problems#problems:tool:db1bf2892de252566cd7219c7c054c56115cc9b26b82cd247bd66388521af771
                // @lfy def/mcp/read.lfy:problems#problems:tool:87fb1f093e523c06d8bd452dd3daa9a60adcb2fa4c4dd899726161be0c185d3c
                ToolArgument::new("file", "a path relative to the root; every file when left out", "string", false),
            ],
            // @lfy def/mcp/read.lfy:problems#problems:tool:88901ed2e2feae85726424eef060489ae98b454113d1f36783aa11b341a5d86e
            // @lfy def/mcp/read.lfy:problems#problems:tool:7ea2a0f37c0856454065e9eeb1658f25ad2acfead105b67722cc2dc039c5021e
            // @lfy def/mcp/read.lfy:problems#problems:tool:6d5668f30152be9816a0ad67fc9c30c0863004d526f68f20d20a072f51191521
            // @lfy def/mcp/read.lfy:problems#problems:tool:d9b6b1ce215d966d4d49ba31d319199d9539fe1614bed20bc86032b0e2fc28c0
            // @lfy def/mcp/read.lfy:problems#problems:tool:123e5ad677760165c91b58ed682062ff6497cb37eb7c02f17e80bcf7f81ef7ad
            // @lfy def/mcp/read.lfy:problems#problems:tool:0365ce41c179a2bdfc7bb6bc7e6f5a2521574397ecc547a36369152171a66b3b
            // @lfy def/mcp/read.lfy:problems#problems:tool:8cd074b3973e9c4a3bb9a83a787e6370caea26c94222c18023fca79732f472ec
            // @lfy def/mcp/read.lfy:problems#problems:tool:a9b7494a79d88bd889cce42866936bb912530e77684203cac316c431a18f60f0
            |session, arguments| problems(session, arguments.optional("file")),
        ),
        // @lfy def/mcp/read.lfy:find#find:tool:e132ab083f2616c0a9f886e961c7096e1e994b7fd593134d0682c3c14c68548e
        // @lfy def/mcp/read.lfy:find#find:tool:64a7427c36430ec8c9e5b9699af3a44d627b57d784c3448c5797f67490c2afbb
        // @lfy def/mcp/read.lfy:find#find:tool:57389a2d52ac601927c3841a7dbd13f10ca56c231d22e66e3ddf12b8b5954166
        // @lfy def/mcp/read.lfy:find#find:tool:ea2d1bca6af575f658c0f695835faedb4e77f280b10551bf06dcf01d1ae9fbb5
        Registered::new(
            "find",
            "Where things with a name are declared",
            // @lfy def/mcp/read.lfy:find#find:tool:7ecb9cc6ffb66b1e366569590d4060dfdd696066f9463240d0dee9c53d39002c
            vec![
                // @lfy def/mcp/read.lfy:find#find:tool:835a1a009cd5f49f70c7b88b2d718244df6c4a39ed9f73d4925fe13b1231e88b
                // @lfy def/mcp/read.lfy:find#find:tool:c9afb88c885fac20c29b4b48da700972a1882420b759f8d79b25d77630a0e168
                // @lfy def/mcp/read.lfy:find#find:tool:26960caba8b776d48816a31b7cd3d9d20254e141db7595c95945236688b3ff13
                // @lfy def/mcp/read.lfy:find#find:tool:01a20a244cc7cf7bd12c17e49fbfebc0c04502486559072c3783e51b53a22130
                ToolArgument::new(
                    "name",
                    "an identifier, or an owner's identifier, a dot, and a member's name; a file path and a colon before it limits the search to that file",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/read.lfy:find#find:tool:4d9398185457aa7fab84df6408fca264cc40ac9c40f927ce96e5873ee5a87493
            // @lfy def/mcp/read.lfy:find#find:tool:0da0cb212aed7b675bf8a12558f696ff9fc2926351e843f06741a1d48dcf2e23
            // @lfy def/mcp/read.lfy:find#find:tool:f8dce49696575cb028eb74d284baa5dbba8b29514ddff662ee8900b968dd66f1
            // @lfy def/mcp/read.lfy:find#find:tool:9e6ccee8bf3cdcda1e9f14fb07ef0083f6b49f3e910bddf0534c5a120eb4f11a
            // @lfy def/mcp/read.lfy:find#find:tool:14b1732157522ebf4b24b6038fcddb65b5b23a3689651521b1bf763ca8fb0879
            // @lfy def/mcp/read.lfy:find#find:tool:26dc105fcfc8d9cc4ed181d1ae1172c0d0e88942453e1513fd22d3d6c8b3b095
            // @lfy def/mcp/read.lfy:find#find:tool:ee0ffd83b80c1fa9a6f6446550d0ede648668263c911d25b7cccde3b1c251207
            // @lfy def/mcp/read.lfy:find#find:tool:b969b67f01ae22cf235972b295dcf1c53ad993facbcd3ecfca3694f891b7749d
            |session, arguments| find(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/read.lfy:entity#entity:tool:b14133579f6b9759ae1979e937ff1aaa58a141db5d2437d71d839b46e724231a
        // @lfy def/mcp/read.lfy:entity#entity:tool:e1c69a6fbf06d17784db05e85f146afdc2ed92a34fb43987c806cc9de90c4229
        // @lfy def/mcp/read.lfy:entity#entity:tool:175628e227d6a58160595e49d7da66a0ba3720c6ea864fabcaf7685eb6d6a104
        // @lfy def/mcp/read.lfy:entity#entity:tool:6fb1b9cd738fb7625982664ced1d50b2a1996ba46a7b736de1f0ca4c2e2ff289
        Registered::new(
            "entity",
            "Everything known about the things with a name: definition, type, documentation, traits, members, criteria, tests, references, and source",
            // @lfy def/mcp/read.lfy:entity#entity:tool:29720a02e6e0244d640f8a4c253e92ede320148b52f75a1b5b7eacfe11c2e754
            vec![
                // @lfy def/mcp/read.lfy:entity#entity:tool:ce7d3834e778c9a33a677519c7f05ec96ecf849dcd62a3aa02fd77889652deff
                // @lfy def/mcp/read.lfy:entity#entity:tool:1688a7f2f37257b8e98d373d0e0c42181ec7505a1280455fe7f5f3a63b07184d
                // @lfy def/mcp/read.lfy:entity#entity:tool:dc7ae5ee6d4efa4661b6e14a134daf3b4186a340a242f2c99b8354aa10976b0a
                // @lfy def/mcp/read.lfy:entity#entity:tool:da01a1719cc145a83e84788e0ccacdf684edb378e0cb800425b88629e2ef17e6
                ToolArgument::new("name", "as elfie_find takes it", "string", true),
            ],
            // @lfy def/mcp/read.lfy:entity#entity:tool:3d70810395770f46f4b0341b19b3b5746a6829d42a024bdba5165bc6d4a732bb
            // @lfy def/mcp/read.lfy:entity#entity:tool:b90d61e8f37841867d442fc530d5ddcf92714d7d1d80b844e8c85cf8597e59b5
            // @lfy def/mcp/read.lfy:entity#entity:tool:413c4c7682326a1441760af5984fa5ff4cacad84ceef75ed3d5f1df46112b944
            // @lfy def/mcp/read.lfy:entity#entity:tool:79374b37a4c2b0e653d83228d8c5563ef6eb7a434e367323698fc7c8d6b4ddfe
            // @lfy def/mcp/read.lfy:entity#entity:tool:fb4d5af52d123ee4f2664e89990ada1b015f962994f33159a15b431af33b7090
            // @lfy def/mcp/read.lfy:entity#entity:tool:95f57cc552a1a5cb5ecce19b598d13f87a05c5bdfaaa6938a13310f2508831b6
            // @lfy def/mcp/read.lfy:entity#entity:tool:5829a685241aba8ca4ca4bf3f62ea2b3055e121c981de79669a9d7c3d83770cc
            // @lfy def/mcp/read.lfy:entity#entity:tool:a7c0ee0d3baa0c575fa939c2dc2d147543f450d6e97fe4e0cf89d5de4453291d
            |session, arguments| entity(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/read.lfy:references#references:tool:91b0b8e47d08c4733fa86ad5af06ea3965620c5c00d40d07c8cc7fb921a0a660
        // @lfy def/mcp/read.lfy:references#references:tool:cb492bde6d5a369786317bbcb74ee8376654febd8e1f52e4b7974b010a3f24a7
        // @lfy def/mcp/read.lfy:references#references:tool:2b8a93892addcbfcba7721d58c72423118422837b4989f9c4c085c7482cddaa2
        // @lfy def/mcp/read.lfy:references#references:tool:cbfdf53dbe0d6ebe62a812187c349b933d14488e7b4da9df8e358161ef6a89af
        Registered::new(
            "references",
            "Every place things with a name are used",
            // @lfy def/mcp/read.lfy:references#references:tool:2646b783aeb40a7eebe94f279731de3bd187157a5b6de43e5d2ed7295da143b6
            vec![
                // @lfy def/mcp/read.lfy:references#references:tool:8a676febf5a8ed15bfe4330e5952233928dcf8ca2ce05ea181240c551b835b00
                // @lfy def/mcp/read.lfy:references#references:tool:8c38aea1e82e269279d66e3527838a3ad463fe9f2f5a260edd02048c4e72d341
                // @lfy def/mcp/read.lfy:references#references:tool:5040afcd04817be85669201eb87a8b9d789a85ad095fd2c10fd0fff86814e310
                // @lfy def/mcp/read.lfy:references#references:tool:00801e0b270e823290817c793008d5f0397ef2e68f54a8165f9d83991546a1a8
                ToolArgument::new("name", "as elfie_find takes it", "string", true),
            ],
            // @lfy def/mcp/read.lfy:references#references:tool:578cd15582e96bfc66f2880b2a48c591dbce2deb398f425f9a4d3063183a9028
            // @lfy def/mcp/read.lfy:references#references:tool:5913c8338e4fe8a298fa992c72acace50365461a6be39b24f5e283c0b4247d4b
            // @lfy def/mcp/read.lfy:references#references:tool:44eba5979daecaf73a71db45d29b3241c24d26df19be8c87c04f0776c54fe9e5
            // @lfy def/mcp/read.lfy:references#references:tool:907da8e53ec21ffd2ba78ae2828dc026cfe99c3aa51c848998571262d0c7139d
            // @lfy def/mcp/read.lfy:references#references:tool:aad077310fe2dbc43c8b73a03ab9c9f16da6988cbc12422aa252f25c40a90b31
            // @lfy def/mcp/read.lfy:references#references:tool:dd81133cca58c8ba8fc6b9dc72579d5233719d65c7212530254058936f847c45
            // @lfy def/mcp/read.lfy:references#references:tool:b4222e73bcbb5478afea50f42b71e34c9d17c5065af4fa14863c508322e95c80
            // @lfy def/mcp/read.lfy:references#references:tool:9ae2c8dcc1248d086202d8cd6bc7e4ce571330e106c86a53547eca02254d5430
            |session, arguments| references(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/read.lfy:outline#outline:tool:eb4909f50a27918f8d4090b839b8e84945dfe78d2a8435b6f7292f31da9873f7
        // @lfy def/mcp/read.lfy:outline#outline:tool:b6b8b54cef8bc5d0b4bf310052c258e50aa82540e2c975253cd5bcd908208bd2
        // @lfy def/mcp/read.lfy:outline#outline:tool:69f17896d41fb427aacfe72b29e39086032b94f68fc0d1024abacfaaba1e9765
        // @lfy def/mcp/read.lfy:outline#outline:tool:e3acad11964324b9d465702630424bd224590317efb9fed52df411bce0c0655f
        Registered::new(
            "outline",
            "The declarations of one file, nested as written",
            // @lfy def/mcp/read.lfy:outline#outline:tool:5e24e9b2b20a966af8ef5248c6ed51d7dc711cb99d83e5129cfd35f8c4063037
            vec![
                // @lfy def/mcp/read.lfy:outline#outline:tool:b5be734a5a6b719e237407065190ce1f921c2ce13d33c7d8f5d7a4db8ad6f5fa
                // @lfy def/mcp/read.lfy:outline#outline:tool:c5ef388a3c005c460bf2f607ced79a87ed09da338da69eef72bb3ef4e56fd31b
                // @lfy def/mcp/read.lfy:outline#outline:tool:b6f44f85d33e69eb9cd8fef87a44c3f84d930895c2a8fdbba60a2336ae625a4b
                // @lfy def/mcp/read.lfy:outline#outline:tool:c981cc2af2eafbd212193c4edd589306b5fc1b12afbf2df7eb4cc1ef1dfd1306
                ToolArgument::new("file", "a path relative to the root", "string", true),
            ],
            // @lfy def/mcp/read.lfy:outline#outline:tool:56807c149fa490c87e28f903d14a7c0f45d7bd9ef10df81481e83b233f241b30
            // @lfy def/mcp/read.lfy:outline#outline:tool:9912a2923d9900857a75b4e16c297f8763189871aec10ad9b59a4c3da8441630
            // @lfy def/mcp/read.lfy:outline#outline:tool:a2fc4d8d3c79341b031d9456a187c8df308d0051f1a4aff47ab6111fdba562d5
            // @lfy def/mcp/read.lfy:outline#outline:tool:f5e2e8d44cd795aa69322fef5a2f21e8c943fd631ec83fb04f50665b2a0d412f
            // @lfy def/mcp/read.lfy:outline#outline:tool:6e95ab5fd88f41d7145f86ed344f8156d3464326e598b0ac637ac3ddbc5d369f
            // @lfy def/mcp/read.lfy:outline#outline:tool:150963511f1c73e51401709017f75ec80e37e411880f968dadf301ed995494cd
            // @lfy def/mcp/read.lfy:outline#outline:tool:6114e21db6cb9375ba623e9ff0b359857133d98565b50e37b08ee737a3e0ac41
            // @lfy def/mcp/read.lfy:outline#outline:tool:532b2ad88427069a8c24f3a734fe21c29f9d0b4bcf57c523a2436c4d0228712f
            |session, arguments| outline(session, arguments.string("file")?),
        ),
        // @lfy def/mcp/read.lfy:grammar#grammar:tool:c3c16f96ee3cf86971b4ccd7fd1a9c9450ab8f70516c3d2c4eaedbb816b2a2f0
        // @lfy def/mcp/read.lfy:grammar#grammar:tool:867847f2e6eb9134fd409375bb2a074218ed073c91533dbef7939b3e214305b6
        // @lfy def/mcp/read.lfy:grammar#grammar:tool:c5653402025c8545919d8b3c15e8bce1041d5302593e40a588969af7a8e79961
        // @lfy def/mcp/read.lfy:grammar#grammar:tool:269dc484865babf40b3e114fadf80bd330d3279d8933541dc768d44ac91dc1f1
        Registered::new(
            "grammar",
            "The grammar of Elfie: every terminal and every rule as EBNF, for an agent writing or reading Elfie",
            // Its one parameter is the session, so it holds no argument, and nothing further
            // is asked of an argument's name, type, description, or whether it is needed.
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:7c4b82dcd83184afd88477db1f841b99e09a48579fbd8a607ecacab9384b07a8
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:f850ab1c3e6cc60934d83c21bf8c38985073e480f783ed7b1e3938becac79470
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:f19df1b9d30afb44626fbac447fc8174ccf96090af4d4c55d7cd15962678ec16
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:1bee4ee804f7710cdf19b8f85355a3d25ef669978aedd26b77fd12b4384bea46
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:6fc2ab76fe5f882a5659db4de547d76cfb7a2d744ead1bd45cf7df62cd222a7e
            vec![],
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:f115d229e0e441100f57379c4e9320b7a35a8a4df3397e7b94b5dc042fad25a1
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:445bc95fc02be39bbfa3ac79b8d2d20865a5992e51582c1b2fd9f0701fd2a742
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:6f8d7a1462a99bf3fa72427d3a73b0a614fdc10fc808163f370626ed4a92c4b3
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:bd5a9029211a8e2f3d3c41e446aba7cfaa1aa0a444f42df76c2d0255eb780df8
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:b32976cb22016686217d233fb9df1603beb2140132b48dd9c7812b79c2173d4d
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:4dc3ad8c479a0c30222640c742e8a2d038c354b7c49002f7e7418207a552f774
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:1e0a1ad9253f2d958a5b86d8cc0eafcecedb8d8cf8244049d2761ea7ee21559e
            // @lfy def/mcp/read.lfy:grammar#grammar:tool:b7323dc3bdb5a4319278e80e814e49181832a99d94e88ba1d9c7a9a3a6d8b0e8
            |session, _| grammar(session),
        ),
        // @lfy def/mcp/read.lfy:units#units:tool:2ac75ac99fe508b7a8c4a2d27aca12fd9d96cccf9950c3f94cce20f1da99655b
        // @lfy def/mcp/read.lfy:units#units:tool:11b09db1a7752a1e56b444520139ac0f584b54714e72d5b267322c54277ecfb4
        // @lfy def/mcp/read.lfy:units#units:tool:9d5a244e3de0666f68b9af569acc4864151e7fd8a5d48f7cfcc6bcabad9e2670
        // @lfy def/mcp/read.lfy:units#units:tool:83745c8817de95a9ccf14eeacbaffaeedde2a6b3dd6b882eb51189198589436a
        Registered::new(
            "units",
            "The units the compiler would produce, in the order they are compiled, and why each needs generating",
            // @lfy def/mcp/read.lfy:units#units:tool:9e4e0920c3a77f7c24ca79866264ca5ea4507712f18a471af4347d4632455852
            vec![
                // @lfy def/mcp/read.lfy:units#units:tool:4f77f81c4f6b8f1b069afb12ae98bb421ef8b1aeafd78e6382ba0c3bc37144a9
                // @lfy def/mcp/read.lfy:units#units:tool:f825dc9832555a4eb351f2e0e489d07be3fcc6600371fdc9bee6bdd83b3b661b
                // @lfy def/mcp/read.lfy:units#units:tool:ca98b07c5016189c3ac214fd187b84a997e536edc1ba4d14c833d955e423de52
                // @lfy def/mcp/read.lfy:units#units:tool:7049caa73147226f8d26eb3c3b60ba88b5cf9217fc22308e57f0cdd5b4a92743
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false),
            ],
            // @lfy def/mcp/read.lfy:units#units:tool:c0392f44087e7b29669f0f8d6b5d294cb0195675616839b4f6d80e791125f939
            // @lfy def/mcp/read.lfy:units#units:tool:f26dfeee8e2b533ee1e3670612cb4079764032f285d8b25f9249a136fcbc513d
            // @lfy def/mcp/read.lfy:units#units:tool:a71b6d7d974a07b20d3d3a0a6bed025139d2e10a00a2b42542d18d13ac16d6d7
            // @lfy def/mcp/read.lfy:units#units:tool:59e3afe0a4f39564b82a347fa5494defa7421ae95a945c19049ed8e307b1b0e8
            // @lfy def/mcp/read.lfy:units#units:tool:6fe94ed319295931073848d26163e8ac3c9efe7cf750da516cb1583b09e4c940
            // @lfy def/mcp/read.lfy:units#units:tool:a2e1c4ab8495bfed9ff4f22428462d6c61f0c1df2bb61f7b05ccacd5e3106f75
            // @lfy def/mcp/read.lfy:units#units:tool:81aae6abacf07fa5edf743b82fc37586ea135ec9ec9af2639c6e6662a0c9469f
            // @lfy def/mcp/read.lfy:units#units:tool:1ee7df6703e37c15ef56982f86d76fae7deacf742479bd28a9b3bde7ee66a1b2
            |session, arguments| units(session, arguments.optional("target")),
        ),
        // @lfy def/mcp/read.lfy:request#request:tool:beadebf1d4caa2b6158bcb4afe387ffbf1fdaa65b6ebb342c852e52d52b2b658
        // @lfy def/mcp/read.lfy:request#request:tool:3d666a1c238113290ac9bc99eba6f8b431a32dfbb44f42d7b63e635a6a487ba0
        // @lfy def/mcp/read.lfy:request#request:tool:30eeb76061a9ac9849a230ebdd0d367531f04ba72967baee676deb1bfa0b711a
        // @lfy def/mcp/read.lfy:request#request:tool:779a0f4249d6f95d5f1ab566cbbd6a918eebe344c981514e7c2ea87f0a639e4f
        Registered::new(
            "request",
            "The full request for one unit: what to produce, where, every criterion to satisfy, the edits other units made to its outputs, the later units it may edit, and how to report",
            // @lfy def/mcp/read.lfy:request#request:tool:79078aaecc3eb0b5da33d600e95bbc7dcde986688c474910707530f5666a030c
            vec![
                // @lfy def/mcp/read.lfy:request#request:tool:a87c03b34486d18a1c17ceb833c722c4dd3842a6a421881c70f322337e0e0481
                // @lfy def/mcp/read.lfy:request#request:tool:5f1390725bc367714b87749e253f03091c3893e803feaaa995bfd7aaefa84e7a
                // @lfy def/mcp/read.lfy:request#request:tool:3960bc68d750e3fc1407e6c8d636ec1b5b2b1066b7a1e809a5ffeec14a2b3011
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true),
                // @lfy def/mcp/read.lfy:request#request:tool:f30b7bfcc6ed637477e5d9c6dde218d0f7b7274a7eb40565013244416b0fa956
                ToolArgument::new("target", "the identifier of the unit's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/read.lfy:request#request:tool:d360786cef01769989e63abe1e561a739a3abcc849ac2c13a4aec96f220addff
            // @lfy def/mcp/read.lfy:request#request:tool:e5bdf55db701cf680b09a00655b7f1d5a6a54182089d06b9ae4d6c4a439e676d
            // @lfy def/mcp/read.lfy:request#request:tool:b7c905f3a462069d30d33e3a3e61cd57f34c3039bfa131ae26f1d852fdd2070c
            // @lfy def/mcp/read.lfy:request#request:tool:6937dbc72b50f20142642a70d702a97291567a2c235b538ce49149e7311ef28c
            // @lfy def/mcp/read.lfy:request#request:tool:9fb364f7e6ba15fc4a1310334b46e7caa824e8536933e4d30e126523ca55344d
            // @lfy def/mcp/read.lfy:request#request:tool:f1c8ff30fb778df56f920993cac9f434ca7d05174e08a4529c76ef50f0f34122
            // @lfy def/mcp/read.lfy:request#request:tool:806f418c79eeea1378264a7f704dd7542b5fafffc84e9d62cd6694870e3f424c
            // @lfy def/mcp/read.lfy:request#request:tool:b0e0cfd7d7f475f1bd8b173c972e686a43cabef5a4a9cc18b77eab7a994d2b5b
            |session, arguments| request(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/compiling.lfy:task#task:tool:b4176adea23f8e6c58f4d041e7a85447286076984bd05464e92528b60fda6d8b
        // @lfy def/mcp/compiling.lfy:task#task:tool:b4a5feee0df4f25397442a14ddc5789cddaecd15441de63010655c2768703e25
        // @lfy def/mcp/compiling.lfy:task#task:tool:46ad78939d73914a576d0dce0904cae992474f9eeb184c717a5a0c324091d4d5
        // @lfy def/mcp/compiling.lfy:task#task:tool:18b0c73855584e9caf73515d1acf890c0655ed42c042d108a8914180338ce2b3
        Registered::new(
            "task",
            "What this compiler run must do: its unit's outputs and reason, the edits other units made to them, every criterion and test to answer for with its id, place, and text, each marked new, reworded, kept, or unknown, and those removed since the outputs were accepted",
            // Its one parameter is the session, so it holds no argument, and nothing further
            // is asked of an argument's name, type, description, or whether it is needed.
            // @lfy def/mcp/compiling.lfy:task#task:tool:6ddac2bcba95be32011edabc6d0abbc63da57cd4a0182a8c86f882a8482706e7
            // @lfy def/mcp/compiling.lfy:task#task:tool:e91db1a6069241eefc991fb1723f76dac6a194f33db9dbeaa21f8163f69cbdaf
            // @lfy def/mcp/compiling.lfy:task#task:tool:b2e2fadd9a1cd573a4b55e2e77fbd573ec3a9f6184ccd88beca5e743b871e9cd
            // @lfy def/mcp/compiling.lfy:task#task:tool:efcc6222428846670162553eed20dfde5c5603101a6a03e991a434832f04894b
            // @lfy def/mcp/compiling.lfy:task#task:tool:b361de400635b42112cf8a02205ed19f3875a7f7f9835b646817814b39f0233d
            vec![],
            // @lfy def/mcp/compiling.lfy:task#task:tool:6906aa388a4da929b046d4c96d4a070b0963d06bccb58ae5b990c00788627817
            // @lfy def/mcp/compiling.lfy:task#task:tool:b5c70f578c9857ac3ed4a51529ca0521467c10eab0b4ab4cf28cc3781d492359
            // @lfy def/mcp/compiling.lfy:task#task:tool:fd081eb82b1124fbd1ca009325b4978d8b342b69049fb32f4cacb808670051a7
            // @lfy def/mcp/compiling.lfy:task#task:tool:550a53793d91d79739538739bb531ddc98c3305e26aab4b81984562bde2f013f
            // @lfy def/mcp/compiling.lfy:task#task:tool:07e54abc770f1a3ac7809eca72a5406d78899640f48cc32a1a8f4f8aeb365374
            // @lfy def/mcp/compiling.lfy:task#task:tool:58f60da4f6fb7b5f0f9d9ded72cb43dc5df3fb5021ebd4dfcc459110960d3243
            // @lfy def/mcp/compiling.lfy:task#task:tool:4edb5fa4433e757f9e3a630b34a68b8a9e99be901d69ac1d91725b52b9682e49
            // @lfy def/mcp/compiling.lfy:task#task:tool:3824695bea06c924cb4eba5253dac7d204a002a30490259b15481ca71f9c4933
            |session, _| task(session),
        ),
        // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:4fd4dbdb36a4d1d4e8a9589471a29d8337372b850160162b8a14f940ad74bbc6
        // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:c19bc9c85c771a6a121e931a3d90f5e925e08c8d87bdaf800db6922b0e2876ff
        // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:7b7fe0f695f34ca950fbd8f4bc3aa16eb90bc78d78ed5a87812fe95b03c71eb5
        // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:6cb58ea6b9b30b6f2aba6bd276fdc3a0e104710c2733f9df000387e646050f34
        Registered::new(
            "dependency",
            "What one entity of a dependency of this compiler run's unit offers its dependents: the dependency's unit and outputs, and the entity's kind, definition, type, parameters, output, and members",
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:01c082cb2c776a72049a0c164c556b408ada68b6c8d3e961aed6be20476ba114
            vec![
                // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:feb08658fc3d60e206035d80439de4021e443fe53d2226e6e8ad43298da9f5b7
                // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:4894a15426436158e1d439a6ad82b49784af28fc4c896d683eb4bcfe455480a0
                // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:2b186e60bc3e791246ddc810258418700586e003d4998497bd580b40f6624761
                // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:81c9fb533af066d41280087dd8b620bc5dd294bb4ca35adb668ec0c9d36c003a
                ToolArgument::new(
                    "name",
                    "the identifier of an entity of a dependency of the unit, as the request's interfaces part names it, or an owner's identifier, a dot, and a member's name; a dependency's file path and a colon before it limits the search to that dependency",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:9dac3037592a32302560384e3c8a39ca962001113ac59a8acf2c5c4780f71e38
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:375badf5392deddf1689b695d0c62a631d22d6b39064e37883f362fdd24b073d
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:1a71bf120077fba410061a19731e2eff14fe02eae36f6c4d869ab1b6d22d664d
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:07aec05fe6ccc4299e78e4c5d9be92049d849a42d2bc4b3ce3e7f2215c356db7
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:cbb88fee807215cb82267d268f14bfd999720fea12dbb9b3088ea6a7734c1def
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:649a317a7da320bf4282cbbf655cddce41a07fadfa4f9b2fb6a7c2d00814b14b
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:730e9d6821325999ab31904691e3e1a7013318bf229e3b12c01b8c8b519dce62
            // @lfy def/mcp/compiling.lfy:dependency#dependency:tool:597317caf65eb0dd18b63c0849c00451c0667a3796442dbf65f7c3e79567420d
            |session, arguments| dependency(session, arguments.string("name")?),
        ),
        // @lfy def/mcp/read.lfy:check#check:tool:6ab2c1cff2807e117f8a6061395f3550cc0741100a599c11fe1972c39b35dacb
        // @lfy def/mcp/read.lfy:check#check:tool:d1b4319d34a3d00324ec0850bf1e0987ceb4112aab80f13d77b82554c27ca5f9
        // @lfy def/mcp/read.lfy:check#check:tool:5efbbf0ff43ad79f41d3ac7bf5367d058f6765db795a0f1f4662585a08da45fa
        // @lfy def/mcp/read.lfy:check#check:tool:9043fbf64929cb4a1d5e4a012d96fe8e84677ea1faa807b230616a4170b41143
        Registered::new(
            "check",
            "Whether the outputs on disk satisfy the request for one unit, and what is wrong when they do not",
            // @lfy def/mcp/read.lfy:check#check:tool:e7f686636ea7a27002b3e7592c7dacea4aaf4782ad7c636ed061a34eafb74fd2
            vec![
                // @lfy def/mcp/read.lfy:check#check:tool:20606b167e31f14c6a9694e62792af4c2d5ba24799dc24be9d5fb029dcc551c1
                // @lfy def/mcp/read.lfy:check#check:tool:c8ac27693e52973e511d6eedac2afb8cf65dcab8e12698a4c0fb63cb79630cc8
                // @lfy def/mcp/read.lfy:check#check:tool:66481f10c2ad4b9468770c8845da8aadc1620c975cb26ebba898b9cc49091033
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true),
                // @lfy def/mcp/read.lfy:check#check:tool:d240d07696d43d22eb9adbfb087c059bc742e7a9cb1d1fee32ee4e86cb1b141f
                ToolArgument::new("target", "the identifier of the unit's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/read.lfy:check#check:tool:775a7d5308dcc9bdbf4efc9c0771345d75c0042b2dcb90727173a98c7ad8c576
            // @lfy def/mcp/read.lfy:check#check:tool:171e2f1ce748190f5ccbf577a5f91fc2fd8683a6b877bb654a16a48965e01265
            // @lfy def/mcp/read.lfy:check#check:tool:15fd0657dd9bfbe4565283f531743c14447968c8361b95ba4e874d2a8309c888
            // @lfy def/mcp/read.lfy:check#check:tool:551327521f11195dad0acd8b202685f4710f5c12578520e07845c6e3f9be0f9b
            // @lfy def/mcp/read.lfy:check#check:tool:e42f51f8d640acef564760981510ea1aa80bae4c7719847ff7ec3001be3cdc31
            // @lfy def/mcp/read.lfy:check#check:tool:684b58b87a0a19dc2fdbf39f7e588c713ffb806ffa8ed9df048eb64354a7be4b
            // @lfy def/mcp/read.lfy:check#check:tool:2feecd005d1f5b032cc009fa850348ac4256213d1b2ce6bd9bfff1963661d055
            // @lfy def/mcp/read.lfy:check#check:tool:d01144c9d33cf8a9ac5454fa795448019dc9005930ef3c1f9e38f1bb5e2ef5da
            |session, arguments| check(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/compiling.lfy:reach#reach:tool:1e225d7ed7e538bb99a23803976c607a8fb5b5c2f2f590ec4b06f35d41697b9c
        // @lfy def/mcp/compiling.lfy:reach#reach:tool:faaa38b77c25cd822a66ebe6d91212142f18b490b4ca2097242485bfd37a3437
        // @lfy def/mcp/compiling.lfy:reach#reach:tool:c88a7beb004c86a4f071d6b34d0c5f3d7912b0665b861ae6b5c12672faa877b7
        // @lfy def/mcp/compiling.lfy:reach#reach:tool:2e8fc71c8da38feba6bff1f4f03c9f549b9f90679c751e836d38594e287fa47a
        Registered::new(
            "reach",
            "The later units whose code this compiler run may edit, the regions of their outputs, and what is left of the fix cap for each region and each unit, given the outputs on disk now",
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:11bb30218cb5fdaca02fd7bfbf8a818e31feb7fa0b019e93a7b4cb2cf9f27dcb
            vec![
                // @lfy def/mcp/compiling.lfy:reach#reach:tool:2c161c5ba7dd6373f653dd48a6665ef2ce0c2fab2ec62d0e8ac6fdea4a7c9eee
                // @lfy def/mcp/compiling.lfy:reach#reach:tool:387bcefdfef41b4b41da6c261feaee0ff6ca9b5b171ad6ae78ac06456558a569
                // @lfy def/mcp/compiling.lfy:reach#reach:tool:3a316b181773ec5166e0204d4db9a4b35b6e70aa0c9c8e5fc7a00ad550ab2495
                // @lfy def/mcp/compiling.lfy:reach#reach:tool:0be8d749d8b083dc788a187d2a6411bd4e5cfd6e62a4371e69faffd3064ddae3
                ToolArgument::new(
                    "unit",
                    "the stem of one unit of the reach, as this tool lists it, to list every region of its outputs; when left out, only the regions this run has edited are listed",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:cf669edadd4a632bcc7c2176dffee7e46a2a03d8239895d42c37a23511322b3b
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:9542455787b60f32b85812fc59506f3a7c0ad77a15ba386b2cf5402ad6274222
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:060d86828005f1f0abb8bae05e593348e0323a9f250c68540d5d0f0f9e9ad936
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:a4f64372d4709087e996bb86f12b4ad75e815f3e1faac2a4f456c147054501f7
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:efc8ae8fec0b77a9f37462b4b357aa384d2804bca39e4765581568a53706a841
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:8e83995ffaa772bb7ede232b07285df6517aa38b607032e09f3f8820e722c254
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:0c890c185e09e1671362d8c1d0c845add38859d2e771a04dce86c973e9f94b44
            // @lfy def/mcp/compiling.lfy:reach#reach:tool:88f208d93a8676710a80c74e023c08abaf01b226dd6b5fc6d7bd0a86e4eb3fe8
            |session, arguments| reach(session, arguments.optional("unit")),
        ),
        // @lfy def/mcp/compiling.lfy:touch#touch:tool:4da83db2b1e129fa631e9e83c60e0103445ac7000e8c574aae3c464bf2eca7ae
        // @lfy def/mcp/compiling.lfy:touch#touch:tool:0953bf28021ac288a8c246b0a2488bbba9357be8c1477cff0f9155da3164508f
        // @lfy def/mcp/compiling.lfy:touch#touch:tool:75d192739bfcff082aa5ae79c7958de50175542afac2af2f3f957082f35312e3
        // @lfy def/mcp/compiling.lfy:touch#touch:tool:929c51527ead304832de6ace9648ad9abf7fe01cc36bcde2b44dee920a36bf9b
        Registered::new(
            "touch",
            "Say why this compiler run edited the code of a later unit; every unit whose code the run fixes needs a note",
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:cc3c84452d493bcd511b8dae121940bf52488fe22fd138b237df550d03be2803
            vec![
                // @lfy def/mcp/compiling.lfy:touch#touch:tool:776fa92cd68713d3f7c57c3bfd278e5e3568c143ceb52276b2acbf0ffe2cd9c6
                // @lfy def/mcp/compiling.lfy:touch#touch:tool:2d9d1a704468bd451112c4cfaade9a8da60c7d3114360580e3e9544020a2d389
                // @lfy def/mcp/compiling.lfy:touch#touch:tool:a0970db31ba3316fc4048021908a1353da824324e5a915be3be50a3018474644
                // @lfy def/mcp/compiling.lfy:touch#touch:tool:8fc3eaaa7c49a72888adcf36143f40522f91d1a89018d230d011b9399aa5343f
                ToolArgument::new("unit", "the stem of a unit of this run's reach, as elfie_reach lists it", "string", true),
                ToolArgument::new(
                    "note",
                    "why this run edited that unit's code, in one line, such as what it renamed that the code used",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:d2d3dd3581a48e716395f2ef9512fc725cb3e1ab34d1536c1602fe2034705fb2
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:8b9651d592d213bcab33ed064cc82ea04ad50730d956a1b77e72f822cfd02061
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:92f681e36b786f07826c5718a8a57635e6d36695333808aefd66a3979e5ae720
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:5b70de758ad4d0887721314fb8e9df3831b9cff383c2b290bee1b73f701c8b59
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:84cac8c5630ff84707a281ab90e515a1ad0c417457659abf90ab9006a065e874
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:b6c963576656e5be18b17a54279b541a201c3fc7d3a075f1872d5fe5f9609a7b
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:fe44165a9fad17b614b346e86649a07f649639c176d1dee0e58b9e9c8d527f43
            // @lfy def/mcp/compiling.lfy:touch#touch:tool:7228b831d1f57147133e8d78e134e7315aebe577a5caf03df2c692ba688ee09d
            |session, arguments| touch(session, arguments.string("unit")?, arguments.string("note")?),
        ),
        // @lfy def/mcp/compiling.lfy:mark#mark:tool:0f8e6f40a5d6de867cde8b1be5b3fc853845b786664671c550f9dad09eb01911
        // @lfy def/mcp/compiling.lfy:mark#mark:tool:d0022315029fb169381cd2230a938407632d7d0dd0131fbc83073feec1ae22af
        // @lfy def/mcp/compiling.lfy:mark#mark:tool:0b9567706ca9a8b66a0757d097e5fd9c7e6df936098d4aeb7f7e6834a6635d71
        // @lfy def/mcp/compiling.lfy:mark#mark:tool:5295f5acf77612da1ff8627c746c87ccff5a0dbb1770c05ed869d1ed28d6c7a7
        Registered::new(
            "mark",
            "Write a marker line into one of this compiler run's outputs, or rewrite to a new id every marker of its outputs that names an old one, so that no id is typed by hand",
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:44369bf678415e07ca9c08b82b57c5fe7969e2e9850e765a8f32faeab4b784c4
            vec![
                // @lfy def/mcp/compiling.lfy:mark#mark:tool:644ced5c6aeca4f5ba2d0a64656faf7dc4224aa51d7dcfc2c46b2d9a241fb040
                // @lfy def/mcp/compiling.lfy:mark#mark:tool:84d31d74a162783af16e3e2c1bbb0339be256b810408f3869f52a6d8f243ec5e
                // @lfy def/mcp/compiling.lfy:mark#mark:tool:80130108b96245bc7a9d383a2cfec314326969ff4054bab4ed3dc8f54da23386
                // @lfy def/mcp/compiling.lfy:mark#mark:tool:745f570e06eb5111681f3c579ae61468329b1f2ae73381dfb2d99e3ba743c32f
                ToolArgument::new(
                    "id",
                    "the id a marker names afterwards: for a rename, the new id; for an insertion, the id of the criterion or test the new marker answers for, left out for a marker naming only its entity",
                    "string",
                    false,
                ),
                ToolArgument::new("replaces", "for a rename, the id every marker to rewrite names now", "string", false),
                ToolArgument::new(
                    "file",
                    "for an insertion, the path of the output file to insert the marker into, relative to the root",
                    "string",
                    false,
                ),
                ToolArgument::new(
                    "line",
                    "for an insertion, the line of that file the marker is inserted before, counting from 1; one past its last line appends it",
                    "number",
                    false,
                ),
                ToolArgument::new(
                    "entity",
                    "for an insertion, the entity the marker names: an identifier, or an owner's identifier, a dot, and a member's name; a definition file's path and a colon before it limits it to that file",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:d98367d940b3e529d2e08602bdac8f47fd16cec96553f895f7018870629df196
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:7ac635ccb304b659fae9f602760e20e353f0dd9965c63cf87f39c14388f43a82
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:27d6627a321b91c18b762a15bd13e0a3acdf4998ac63354bd71ea16d6583b690
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:385acec2857321e10454e5a802b16a10ef28dc7faa7ad1d0368c0be3d5f107df
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:dd84a3f71e4f4f6435a8276ad2cf10761a7252093a95113c1fed9f428efc69fb
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:c460de7b205e88646a18f16cc7fa76d5a2725c3f7b52b73abbc413c79548c188
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:c9cac0d319222b17058eb203cf66e762b5dfcc60d8542ba572c63268cf5d241f
            // @lfy def/mcp/compiling.lfy:mark#mark:tool:7230fe00beab23c3f13fc1407ab1d636b4e55a8a1725832980d751e19d6a1f39
            |session, arguments| {
                mark(
                    session,
                    arguments.optional("id"),
                    arguments.optional("replaces"),
                    arguments.optional("file"),
                    arguments.optional("line").map(|line| numeric(line, "line")).transpose()?,
                    arguments.optional("entity"),
                )
            },
        ),
        // @lfy def/mcp/compiling.lfy:submit#submit:tool:46e6a49028a494b7d6bb872cd0cc63d6d03e25fffce40f625b0facd57b49a725
        // @lfy def/mcp/compiling.lfy:submit#submit:tool:8f4adcbdfc16d749d9b83d55424fdb414dca3efc886559174e902fd2a4912a79
        // @lfy def/mcp/compiling.lfy:submit#submit:tool:46c97b8c41d5f41394e9c6efbaf1f3bdc24c40082759615fb1b06732fb2d02c7
        // @lfy def/mcp/compiling.lfy:submit#submit:tool:59836951e15ef484cb381d25b49b247ff2da58ae11631e91ff190839871ce24d
        Registered::new(
            "submit",
            "Say how this compiler run ended: done when every output is written, blocked when it cannot proceed, or clarify when only a person can decide",
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:27e3a76662e3a4a2d2275aa703d2b835bd68c51282753e21b9337c744d37e5d6
            vec![
                // @lfy def/mcp/compiling.lfy:submit#submit:tool:87847e0f685515fe21c083343037e22e14155b3956e0220e0c6c17c145fd682d
                // @lfy def/mcp/compiling.lfy:submit#submit:tool:f5d375e7f92630c7c75c6af5a62d636df1c895f2abf6280b5fc27486624dd3d6
                // @lfy def/mcp/compiling.lfy:submit#submit:tool:5e8f39a0f84b057b8516d3aba402f08b7a867533e314d8ede4294d9169055ec1
                // @lfy def/mcp/compiling.lfy:submit#submit:tool:dd8cc3696294c48c92600acd3e1cc71e6e50459a5f971d53856d37d8d18509ec
                ToolArgument::new("outcome", "done, blocked, or clarify", "string", true),
                ToolArgument::new(
                    "message",
                    "for blocked, the reason, quoting the criterion; for clarify, the one question; for done, a note that is no part of the outcome, or empty",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:e1cdea82bc8c49ba5d42b1a183d864e50dcdcb79c278c71097f6e1c6635ee7ad
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:61ef1fd9f675009231990424e1bb2d2bcbed26bd90a15c89262acd18f8d3aa64
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:b04c384cd1c006ae6ca5c0a481d033184e71884090bfd8a530d95e2124ffbc5a
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:f87ad8b66f74c9462b6bb314454e9746aceb34d3fc56ede64867e6767e61b95b
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:69f43f991763121fd742f21cdb9b3ca44378a49aa50d5a029da01b958d61fe75
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:4c16156f137c18f569c4b5529c1eda802ccf1aab677a95ef553a87ea8267602c
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:f883f2fc8ea86956d67e88fcad8d1e9d60d6e28fd202faf0d872b9c7d78170b8
            // @lfy def/mcp/compiling.lfy:submit#submit:tool:8c456bf550d67a36f52300668226f9b16da8299ec30096adc20c36603993cf04
            |session, arguments| submit(session, arguments.string("outcome")?, arguments.string("message")?),
        ),
        // @lfy def/mcp/read.lfy:output#output:tool:6ec90ebb27e8df39d72c6327a86558dba615e33de7e1a1320cf3f20be8be9b9b
        // @lfy def/mcp/read.lfy:output#output:tool:4eb131ab31bb445c54c0ce94c545024446ab993437862448bc07d78bd45c18d0
        // @lfy def/mcp/read.lfy:output#output:tool:c72de76b2235f6d72935b0bb9556fdcb35f808b7c57557076b5abef04e1ae663
        // @lfy def/mcp/read.lfy:output#output:tool:d72cb21d0e903b84ed1802adf513079764243da9561991d7767a50a7eb2e2cbd
        Registered::new(
            "output",
            "Where generated code is, with its text or without it: every region that came from one entity, that answers for one criterion or test, or that one output file holds",
            // @lfy def/mcp/read.lfy:output#output:tool:2e6b42c0a83704a168683bcc35f00029ab55fcf28d2f29b1c3de09b006c0a9da
            vec![
                // @lfy def/mcp/read.lfy:output#output:tool:b44f0f08cd4a5a17ed3563ee169631ec0e06701a1866f2bcbb070d4ea652c7d5
                // @lfy def/mcp/read.lfy:output#output:tool:536b816ae7d597b602e24a5aa8afccf72252f366fdbf9adb3e6ee1abaae424ac
                // @lfy def/mcp/read.lfy:output#output:tool:84805fed91235a6a28f19e8f6aa8a7e0de24b33cbd3ef8f1ae5312db98688ddf
                ToolArgument::new("name", "as elfie_find takes it, a requirement id, or the path of an output file relative to the root", "string", true),
                // @lfy def/mcp/read.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
                ToolArgument::new("target", "the identifier of one target; every target when left out", "string", false),
                // `text` has a default, so it is not listed as required.
                // @lfy def/mcp/read.lfy:output#output:tool:cccf2bf991122a41e5f8d1b615ff93f9a9b92d85f03790b2610579b50c0becde
                ToolArgument::new(
                    "text",
                    "whether each region's line is followed by its code; false gives only where the regions are, an outline of an output file",
                    "boolean",
                    false,
                ),
            ],
            // @lfy def/mcp/read.lfy:output#output:tool:c74c2a14496848786c2a9b0c1df1da9d6b9205300b34a2f42d670ce562aa0cf3
            // @lfy def/mcp/read.lfy:output#output:tool:81224c8b94d715808fe36a32d50ac22cb7b68f3aaf9efac14a04a2d3ba729de1
            // @lfy def/mcp/read.lfy:output#output:tool:30281c2c4ab3871ba9d3e8e1d4bd3865e4ffa3438c789f65733035c5008a4f9b
            // @lfy def/mcp/read.lfy:output#output:tool:3e79b4342df7d8ac276a343f02bf39ffd5636a96ef3ba4641642d00b806447e4
            // @lfy def/mcp/read.lfy:output#output:tool:76a973ce9145a4809d1d1de1a68875e33067542ee119844cf65ac55c2d7affca
            // @lfy def/mcp/read.lfy:output#output:tool:1e4baa6263f16aec686a93f72ebe9e48822140f1dbbcc30be00356c10db27e08
            // @lfy def/mcp/read.lfy:output#output:tool:1efeaccaafd14ce38d6aacf01412cd2561c720263e5fcc9555e78c00c0bc642c
            // @lfy def/mcp/read.lfy:output#output:tool:355f2e4745bc2e753294ba137c3f91cae87ebf6bc265f34a566e5fd7b744b62a
            |session, arguments| {
                output(
                    session,
                    arguments.string("name")?,
                    arguments.optional("target"),
                    truth(arguments.optional("text"), true),
                )
            },
        ),
        // @lfy def/mcp/read.lfy:search#search:tool:38ffd83bb0a3a18ad44b57a02e041c06b560ae946cbab843e22f39a99e9aac66
        // @lfy def/mcp/read.lfy:search#search:tool:d2c7b2635329f74a09d591b5a08a10bf91aec95b06cf165df0d237df2bfe4442
        // @lfy def/mcp/read.lfy:search#search:tool:49b02b5a6852ab0082ddca33e30f1e4557cf5670633599b4da7d94b54afc74a0
        // @lfy def/mcp/read.lfy:search#search:tool:dffd65949512cdddb7735f1aeed21e452cf605acabf06b395717014263c1c4e8
        Registered::new(
            "search",
            "The lines of generated code that match a regular expression, each with where it is and the entity, and the criterion or test, of the region it is in",
            // @lfy def/mcp/read.lfy:search#search:tool:84b35a27f4c75349de61c3a20a30bb45eead886577edc895af4e407810c893a3
            vec![
                // @lfy def/mcp/read.lfy:search#search:tool:8cabf755d8b4ed9c400ddc760b3676b7d288d84ef6a4b4d3588840ee0fb85e46
                // @lfy def/mcp/read.lfy:search#search:tool:502a513b0a0681b5d9af190aa237ed651ee77b387a6b91c00943b77203be2a26
                // @lfy def/mcp/read.lfy:search#search:tool:1e8cb253167c19b0e2e9148d8b4dd61e332b68d5b13ccf6aa0523572759ef678
                ToolArgument::new(
                    "pattern",
                    "a regular expression, matched against each line of the outputs without its line break",
                    "string",
                    true,
                ),
                // @lfy def/mcp/read.lfy:search#search:tool:32aab10a54d3615f163e0fbc4cd259bb8fdce07cc79bde7fddfbe0fa16fba954
                ToolArgument::new(
                    "file",
                    "the path, relative to the root, of one output file or of a directory under an output directory, to search only under; every output when left out",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/read.lfy:search#search:tool:8a867d96708fad948dfe52a7145345a0d001ac770d16f11460714158c328ec29
            // @lfy def/mcp/read.lfy:search#search:tool:4880440147d84d90bbd981904421e3f06c8c23cac72f4ca0f650d96ae8f43cbb
            // @lfy def/mcp/read.lfy:search#search:tool:49b79ac88e68ac0c8f20e55a2f107e9adcfe236d66979aabf0ee57746aa50a0a
            // @lfy def/mcp/read.lfy:search#search:tool:6181491e41c7534b36495a0ec649e67956a35e780b4963454e225fc0b554754e
            // @lfy def/mcp/read.lfy:search#search:tool:c171f43a45ba4151b6a0bdb64449c032c1bf9926719152f7a9c0142c5de27d5e
            // @lfy def/mcp/read.lfy:search#search:tool:83e2124f3a0907f079adde2fc91d328be754c3ab340536e6154b1c98198a10a9
            // @lfy def/mcp/read.lfy:search#search:tool:215037b5f46848fb934ed18ac570f9ed009eab54e0614959a956d5c99abd3f9b
            // @lfy def/mcp/read.lfy:search#search:tool:d86ec47169d8d453d526a2ae4c472edebcf52bce2a253c8461f5b2a683122d42
            |session, arguments| search(session, arguments.string("pattern")?, arguments.optional("file")),
        ),
        // @lfy def/mcp/read.lfy:source#source:tool:177439079fd384c431d5f00043095ba02ce9d5d960147cc9393b04b553d4026a
        // @lfy def/mcp/read.lfy:source#source:tool:0e837a65b26e2ac3b8f7d64f5dcd769d6785bb879e63c4bd5761e9543fa11814
        // @lfy def/mcp/read.lfy:source#source:tool:5e06f9d1bab537b3f12cc6f40e5b43d875169e6dde2af548117c563a52564a91
        // @lfy def/mcp/read.lfy:source#source:tool:a69258bb42344c0bd286241ad3495c363708649d40587973d2446d8dd7a67ba8
        Registered::new(
            "source",
            "Where one line of generated code came from: the definition file, the entity, and its line",
            // @lfy def/mcp/read.lfy:source#source:tool:86bcade62e065481b0e71151933c5e24311e5ae5c768cf621808ea4c2804928b
            vec![
                // @lfy def/mcp/read.lfy:source#source:tool:c6487b689eaab2ccd0e199ee1b320e9a2665cee02bdd6cb984e65e94e9b22b46
                // @lfy def/mcp/read.lfy:source#source:tool:3e6dbae45a3eab388dff4594bb23659b4840e880ec944f2c8b52d151427f8e42
                // @lfy def/mcp/read.lfy:source#source:tool:5bf72ec14e48137ed89a57ea542187e7e9c485c8a42ba64d90d18cc298f7a0fe
                ToolArgument::new("file", "the path of an output file, relative to the root", "string", true),
                // A line is a number, and is listed as one. No parameter of `source` is
                // optional or has a default, so no argument of it is left out of `required`.
                // @lfy def/mcp/read.lfy:source#source:tool:582533c08bce4d6d3774ac52fd022fbdcbd0c60b58c1b8626ecf62dc622eeab5
                ToolArgument::new("line", "a line of that file, counting from 1", "number", true),
            ],
            // @lfy def/mcp/read.lfy:source#source:tool:e0b14b4706f8dae10a85f002eb93be09aa1f890a3140dca03046fdc0ed52915a
            // @lfy def/mcp/read.lfy:source#source:tool:c88f2812efd1f28c24567ebe4b778a45c64bbbdecc46e8aa143b46ec4fa686e3
            // @lfy def/mcp/read.lfy:source#source:tool:b9fc9d843cff425a9166223d1c90501b53db07c6fa1d662bf901e4c44542d92e
            // @lfy def/mcp/read.lfy:source#source:tool:09d86a7ebefb49c93cedccbd396bfe51c1fd158af3e4aea4f45843eebd4da3a2
            // @lfy def/mcp/read.lfy:source#source:tool:6d9dd51635b7204772d74da138f616b326aac0af4536ec450e8f26c7c9650de0
            // @lfy def/mcp/read.lfy:source#source:tool:f64e48f070058897b4b82d2d2649c81b9155f184931f4b71a81c2bb9163b850a
            // @lfy def/mcp/read.lfy:source#source:tool:36b64abf8443d64fac524ad81fcaa9c9e1ece561cdf7b0a6ff1e66dba001fac3
            // @lfy def/mcp/read.lfy:source#source:tool:933096756b3b232e53313f802792772462a7923e6772c081a67cd7f54440b7e4
            |session, arguments| source(session, arguments.string("file")?, numeric(arguments.string("line")?, "line")?),
        ),
        // @lfy def/mcp/read.lfy:changes#changes:tool:ec6bca8a3cbc36434de614909a53653a3b0f25e84dfad4d7c7318de2bb35879e
        // @lfy def/mcp/read.lfy:changes#changes:tool:0f166abc4aab53e3c0ebb7ea52e8dc8852f3d4d8d10729c84048c79f3b95c964
        // @lfy def/mcp/read.lfy:changes#changes:tool:23f664f16c7c7c3856d8730e9420e96c25250a86fc013c64a2dc86e9ba9690fb
        // @lfy def/mcp/read.lfy:changes#changes:tool:bb175b58bc93d07549300b484ff2382a881c6cf49c12e9210f23f57c0c852709
        Registered::new(
            "changes",
            "What differs for each entity of a unit since its outputs were last accepted",
            // @lfy def/mcp/read.lfy:changes#changes:tool:ec3c7defff9f765cc593846440341f6c9ab6818d3c051ab9101385709365d3fb
            vec![
                // @lfy def/mcp/read.lfy:changes#changes:tool:490033cd4b258e1604cf1c128b2ae6ab1127d571246c9bbddfa64de843ff9ce7
                // @lfy def/mcp/read.lfy:changes#changes:tool:07440d03005075a867105049b30aaf727dfbcb8271af1b1538caca58cf4903ec
                // @lfy def/mcp/read.lfy:changes#changes:tool:0c631194a5026a108cc79d09dc3877b6278e686a4a4b0853d4b441b65c8bb6fc
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true),
                // @lfy def/mcp/read.lfy:changes#changes:tool:7669d2bede3f5a5167af6c96cad8a2292f2328bbbffa4e4e9fbb74a18d346d49
                ToolArgument::new(
                    "target",
                    "the identifier of the unit's target; the first target in Workspace.targets order that has a unit with that stem when left out",
                    "string",
                    false,
                ),
            ],
            // @lfy def/mcp/read.lfy:changes#changes:tool:07f057e8b5e7087629b58f0a25b19295add4159a054a4a1dbb02181687082f5c
            // @lfy def/mcp/read.lfy:changes#changes:tool:1085cc6532a1e368462f1f090f302b718569139cd8f264ab5261a97307262bb0
            // @lfy def/mcp/read.lfy:changes#changes:tool:5226f13af1af781bf79cce55820c206559e6603718d66ea566a8a88c040a2247
            // @lfy def/mcp/read.lfy:changes#changes:tool:f5c4e810ed5d871ec0a3742590856586a4bbb180829fa0a37f5bec6655f6667d
            // @lfy def/mcp/read.lfy:changes#changes:tool:aa51c0384474065c01079886c2fa98ccd1c60e25ba6a48a34cb22571fba8088f
            // @lfy def/mcp/read.lfy:changes#changes:tool:1b481949890f42d1353e622ca00080011c52df05703303c8a4a0073f55d96380
            // @lfy def/mcp/read.lfy:changes#changes:tool:7956a05a34c47e5177fb4661eb64d4384e77763432be57f4b4db3f2d15aa972b
            // @lfy def/mcp/read.lfy:changes#changes:tool:0dc4b671044e5b169084445b22daec236489f564fb85985cca66cc6a88b680d5
            |session, arguments| changes(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/read.lfy:review#review:tool:cae93d1e0915b7e3244718911a45cd0e8a145cb441856eec83c09a11547ff0a6
        // @lfy def/mcp/read.lfy:review#review:tool:70891d9a91ae04731a72b80d2f6c86b173925fc777f95ee77561af6d5b77ef37
        // @lfy def/mcp/read.lfy:review#review:tool:90f43a9d7a341c4448c88bdb65846b1958c620269b4928c1820b8bdd36a04645
        // @lfy def/mcp/read.lfy:review#review:tool:0547cd795e9c7adad3ab04c85d39ee2a7ac5fa01ecebb3c3a215ef45614fc7b6
        Registered::new(
            "review",
            "The full review request for one unit: every criterion and test with where the regions generated for its entities are, and how to report",
            // @lfy def/mcp/read.lfy:review#review:tool:4db83e86e12ebe1d2b0275a0875d451913e2e33a2985e19be1239eeb55afcf55
            vec![
                // @lfy def/mcp/read.lfy:review#review:tool:176e2dae9d3fa2eeb0d9e1f4cb3f69e21eddf6a4ee54eeec769831da57d33106
                // @lfy def/mcp/read.lfy:review#review:tool:f12b54937440941b301f9813502fcf02c0f5c15985fa86f3c18d3be8971e926c
                // @lfy def/mcp/read.lfy:review#review:tool:26448cfe30979765e8825f454475ab554f07d764985e474795c7f379bcb790b8
                ToolArgument::new("unit", "the stem of a unit, as elfie_units lists it", "string", true),
                // @lfy def/mcp/read.lfy:review#review:tool:c9eb9c029bfc76ba91183426864af318ca0ba9c40d66aa878456a1365ec79882
                ToolArgument::new("target", "the identifier of the unit's target; the only target when left out", "string", false),
            ],
            // @lfy def/mcp/read.lfy:review#review:tool:a214aeda7f2dcdb8d57836782e9f7950b99a0cbffd7ded1e4d02512863333bce
            // @lfy def/mcp/read.lfy:review#review:tool:c5cc7af933cc365db492e83cc2677c5381c9cc7293ef97f115d12d762c1b2584
            // @lfy def/mcp/read.lfy:review#review:tool:8c428dc0edd273685caaabe72eb81a080104324829ee2f95f5e3ccab5c86a3ca
            // @lfy def/mcp/read.lfy:review#review:tool:6136567dbb52c10db4b93ba491d3db66273bcb1eb7671f8831bb2f8567157ce6
            // @lfy def/mcp/read.lfy:review#review:tool:1f10b2e2381645fd0db250b709faee6d4fcb7777f16fffff10731a3c197f94cc
            // @lfy def/mcp/read.lfy:review#review:tool:3d16d076268d4bb0d06e54067ea264107a204761e51fedb45c43d4da6266e867
            // @lfy def/mcp/read.lfy:review#review:tool:783352704aa244702e71b187abc170d3a55eb7ee3b01dddbdffd6b21219355e8
            // @lfy def/mcp/read.lfy:review#review:tool:5c26dbd79645c2274940ae02bf61ebc5954a4c194e657a89b203f1976fb73dce
            |session, arguments| review(session, arguments.string("unit")?, arguments.optional("target")),
        ),
        // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:63522dbcfabc1cddee57ee188e7c75c7a4c6576f807ce8a0a4dd4ffe69250ca7
        // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:d895cc0bf43e7d708d5eba5fe5af8ca74d3565a1d0ebba4ffd5d274cb025ebeb
        // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:14933b1ce190a0da8af31a4b31f6a8dc1a38277ec20a83476f9cb01e93f24e8e
        // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:3f4ec62a56477a71ffce1d8e9448a9c31cbe5891bc4426c362c26086454332b3
        Registered::new(
            "globalReview",
            "The review request for every global criterion and test, once for the whole program, with where the regions that answer for each are",
            // Its one parameter is the session, so it holds no argument, and nothing further
            // is asked of an argument's name, type, description, or whether it is needed.
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:82e3fa7ae3f9252bf237de6847f6e4f376d8359dbf5534ae09480fd3423e9c90
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:37bd7f504566a0de5dd33fd249659e845dfa71e53baf0404c88d78abdb413040
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:dfd01992e60e996e02f3abdace15928aa26c171d40598e6ef831122cca7dc13e
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:e6002fa85fc2e5cb7599cd691c03f7c44cd15a3a91606fb73ee48193ac48e0d4
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:5f7ebced7900d0c3e6313eb42c7094e11d5a40a106f18a1451685cf793ba8c45
            vec![],
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:49180ae21ec1d01742b6b16217a91564016033aded3011a92220096a63031b98
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:b965850b0512ad933fd27191c3cf2fb9d268a9f8c7f28a2fa66402f7f6302d20
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:f2d3f884428ba9b23752fbb54426c519c209d4c0804df1b4c254f81834cbe0a9
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:116e7e050e8da1a1cfcfd7ac5bbea519c9e62b5349a6a40b2b8f69de71ace555
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:ddb77eff3445623b782407a0b541cab4cd213bfe13fafedbde6e2c9f1c58926e
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:1bd58ca877dd1efffc6fcfb3da9e0bc8a89f596f6bc8293a4de3c8e6ebe8a903
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:dd97712ea04dd944092a306fe48514d200aca70d8859b91716e78311d351b34d
            // @lfy def/mcp/read.lfy:globalReview#globalReview:tool:a465a324a0a88351aa9aceb3ac8748f741a2f91ba57cccc57bc75ea168bbac9b
            |session, _| global_review(session),
        ),
        // @lfy def/mcp/compiling.lfy:report#report:tool:6c734414d43397d25076d1deee1bd29dc4629bfe66542bddca3ce1cdce3d96fb
        // @lfy def/mcp/compiling.lfy:report#report:tool:ff191fef77edc21f5b1b6379da11a026007625ceb2ea84877365d76b43d3a88f
        // @lfy def/mcp/compiling.lfy:report#report:tool:75d6b6bdeb8d8a74966583e1c58c03775a0f243350447c165a263304032f268a
        // @lfy def/mcp/compiling.lfy:report#report:tool:a1742728abf4993bf168d78ca9c2423c2bc5bf5a43161ca1f56dfa0a9098fc27
        Registered::new(
            "report",
            "Submit the verdict on one criterion or test of the review this session is for",
            // @lfy def/mcp/compiling.lfy:report#report:tool:e2565b57b91b75898a51429de938829bbc20515ebf54db2921b4ff1bf9a54879
            vec![
                // @lfy def/mcp/compiling.lfy:report#report:tool:69094a572908b566940e33fea2e59850769a5b07a88194d58c582bd3fe9e78dc
                // @lfy def/mcp/compiling.lfy:report#report:tool:189e78800b2582a76073a4234cfc05713af072e7c7dedd6248b2388cf0286d40
                // @lfy def/mcp/compiling.lfy:report#report:tool:54a6b370411e4c4595c18f8704defe58865d880d81e0cf482dc2d9f5a4559834
                ToolArgument::new(
                    "id",
                    "the id of the criterion or test, as the review request gives it; may be left out when file and line name it",
                    "string",
                    false,
                ),
                ToolArgument::new(
                    "file",
                    "the first half of the place the review request gives the criterion or test, such as def/a.lfy",
                    "string",
                    false,
                ),
                ToolArgument::new("line", "the second half of that place", "number", false),
                // @lfy def/mcp/compiling.lfy:report#report:tool:cb1276d6f803e4db72b776992eaa443eb5a2a3262545b7da8843f1c39d27ac7c
                ToolArgument::new("status", "satisfied, violated, or unverifiable", "string", true),
                ToolArgument::new(
                    "evidence",
                    "the output path and line range judged, such as crates/elfie-core/src/x.rs:120-134; empty when none",
                    "string",
                    true,
                ),
                ToolArgument::new("note", "why, in one line; for a violated one, what the code does instead", "string", true),
            ],
            // @lfy def/mcp/compiling.lfy:report#report:tool:ba274d3143e08dca992403814eb84d371a75a9ccf10bdc3886b7f4231fee61da
            // @lfy def/mcp/compiling.lfy:report#report:tool:6dc5f130f1e89693dbd6fa0880cdf2ac8d2707f1a139f3fb6ddb612870f43651
            // @lfy def/mcp/compiling.lfy:report#report:tool:e007736a21684f3a220132392343eddcb6da76ee00624a48a375f7ebc96a48e8
            // @lfy def/mcp/compiling.lfy:report#report:tool:47ce9c03cbce9a8af5a39f9f8a921dda6ab3b03d6962aa825201f5a68c964549
            // @lfy def/mcp/compiling.lfy:report#report:tool:dcade66a6165424800631bce9e97b04fd1a999eedb40dd6d92fc4caeb2fcbc1a
            // @lfy def/mcp/compiling.lfy:report#report:tool:24cf190dc5ccc762c260fd7f9a21fc7a10f1379914fe96ad237b9e95a70fcaef
            // @lfy def/mcp/compiling.lfy:report#report:tool:32260c0058131ab628d7a49257a394dc8b025e50525f206439eae3edd222e245
            // @lfy def/mcp/compiling.lfy:report#report:tool:82ad2f8ed7450e51414f398a901f29cfad00fbff59cc1159c376d09c2bfefc65
            |session, arguments| {
                report(
                    session,
                    arguments.optional("id"),
                    arguments.optional("file"),
                    arguments.optional("line").map(|line| counted(line, "line")).transpose()?,
                    arguments.string("status")?,
                    arguments.string("evidence")?,
                    arguments.string("note")?,
                )
            },
        ),
        // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:f01b15561569a0e0f3fe1c807d0090777c65c341f0847081cfd2ca2ca6510404
        // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:a891450fb8dab1c379f80dc00b90a81a08d245ef019ff39a6cfe41e35f341e81
        // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:3be7a6c8b7fbce3aad80a61e86a852b724708b126e371b256a9caf6f36c74cf0
        // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:1f75ffe1a1f121f6eccf5ce01890249dc085c15368a599a189d750a03d38f7bd
        Registered::new(
            "reportMany",
            "Submit the verdicts on several criteria and tests of the review this session is for, one per line",
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:abe567ef9b047229c61db0195c029cb8f620504ce40c17cce83e5a4585c3aa74
            vec![
                // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:3f1403939c30a1d5f4ccab6e44f91391d63646b18fa9af45f024cd25cd933e72
                // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:8bc614e24dcda70c4f2cf5a4dcd6de692d95748944e20e11793ce871c02c86bb
                // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:8108c07f987756a5e23fc7f37652c3f8c504e645a63cac9bdd03289897eddeb3
                // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:5c134db0ab5425e24ea1d02d5ce99320b1af4c5ce83c919200706e743dd95a28
                ToolArgument::new(
                    "lines",
                    "one JSON object per line, with the keys id, file, line, status, evidence, and note, holding what report would be given; blank lines are skipped",
                    "string",
                    true,
                ),
            ],
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:8a9bd98346d73959f55456776f6c9a3674c22f8a7f0745fbbfc7895e3a9af822
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:eb0982bffadedfec2e84e260d8bfe431cc233a1693e27a33c36243d1f50f394d
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:71de755b4ef19af72d409e196368ae4dfdd9f871354d7aebf4fc474c3f4fe45b
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:eaaece5ba86eeaf0fe57c1cb718495e671d5dd20b84ce6687e81eab36b38e68c
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:7d04b7b98f245e611b2c903c31c01074f55d6eef25dff649ec57591e2f247f94
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:d8fa532758ebcb091e7cb466f13625e715b828ec7fc7b0c35fa74b03abdcbe3c
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:efc21ef4516495524dd25dddbc0d787ea0c43dd38fdb60582d533cf48636b4a6
            // @lfy def/mcp/compiling.lfy:reportMany#reportMany:tool:88a6f4d564ceb31a103034e0f12f452b59ab4f981022b09aaa73e298f20a32f4
            |session, arguments| report_many(session, arguments.string("lines")?),
        ),
        // @lfy def/mcp/commands.lfy:build#build:tool:437d060ed5ca74732574dd1afab19a5a166957ef20fcdb97bf4d590041c48336
        // @lfy def/mcp/commands.lfy:build#build:tool:88906b89a3fff75afd55acabcbfcfcad2e07d893ba2ff2c1a9fbe35abfcb1f8d
        // @lfy def/mcp/commands.lfy:build#build:tool:e0309114048649453e6790ef4c6deeabd6ec9481b22aa5d6159c671535d25832
        // @lfy def/mcp/commands.lfy:build#build:tool:d0bfe30fbbef86bd5724c6c59435c754542f0b44ae99df590217de4b3e02fd28
        Registered::new(
            "build",
            "Run the target's build command in this run's directory, narrowed to a scope when one is given, and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/commands.lfy:build#build:tool:a89edaee38beabc1ef0df886f2a21cc45ce985962b64e3309e470a2a02e9b576
            vec![
                // @lfy def/mcp/commands.lfy:build#build:tool:a3290c07f19e4214355190476d5ab8cc6a9bba3ec08cfedee6e90f738f4dcfc0
                // @lfy def/mcp/commands.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
                // @lfy def/mcp/commands.lfy:build#build:tool:501c436862b393c2162f170908d65e859adf42a2afe8973015becc465cc36df3
                // @lfy def/mcp/commands.lfy:build#build:tool:d7aadcec979a675c909a3493228563ee20b979feaf86ec325edc207f6683b4f5
                ToolArgument::new("scope", SCOPE_ARGUMENT, "string", false),
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/commands.lfy:build#build:tool:dc3440b1ed927411929bf777ccdee3fa195a29b2e360b86349621d1cd28f0899
            // @lfy def/mcp/commands.lfy:build#build:tool:61838c6a72b2030685de2c121a5cedbd7fcae01c5f420817b479871235807b2c
            // @lfy def/mcp/commands.lfy:build#build:tool:e12dfbde34a596f9d4adba2bbf56c5c9e8bb66a94ba4d495da554a8e9f24e9e6
            // @lfy def/mcp/commands.lfy:build#build:tool:9c0329a8cedbc80328b8f48979d3e131ae4e06f573e61d535e026d3cc3016667
            // @lfy def/mcp/commands.lfy:build#build:tool:f717af6b96f7f183061064eaf6c5100974aa1a5044351a8b1d1bd849fdff0814
            // @lfy def/mcp/commands.lfy:build#build:tool:647bbeea74883056cd3e033a601636e7830b8834fb1c6959e6d67fc25b2e9a2d
            // @lfy def/mcp/commands.lfy:build#build:tool:3f6b3e002975d4e0f6c61fa73845c14f4bb28aaab5eeca8bb72b042120eab977
            // @lfy def/mcp/commands.lfy:build#build:tool:b896e4ddc78794b2515435559e036b43da3c57899559a9e9fcfab6b19fd23377
            |session, arguments| build(session, arguments.optional("scope"), arguments.optional("target")),
        ),
        // @lfy def/mcp/commands.lfy:test#test:tool:5386b37f95ca729944462379291817c9470858369f6cfb9387b834f8767c2b51
        // @lfy def/mcp/commands.lfy:test#test:tool:3095733c477eec37202566f8813793d01f848b595701cdd835ffc630ba78f768
        // @lfy def/mcp/commands.lfy:test#test:tool:a038afc81a5b6f64a780e0144c3ac1be7ce831938025338a3c18193929918aba
        // @lfy def/mcp/commands.lfy:test#test:tool:2762b11ba85ed708418f9df9c62fc0b7f1cf6561920586cf0b39f730acb05fa8
        Registered::new(
            "test",
            "Run the target's test command in this run's directory, narrowed by a filter and a scope when they are given, allowing tests that fail only at a stub when asked, and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/commands.lfy:test#test:tool:4930fab7d3d2c47253afb07544bc3ab8fcd4c5deb1e42ce3f9f8b2dcc6175c30
            vec![
                // @lfy def/mcp/commands.lfy:test#test:tool:cbeb0da9f28bfbd6a6d084bb140fca7c043af3a7fee4c0c506d08562fcb13392
                // @lfy def/mcp/commands.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
                // @lfy def/mcp/commands.lfy:test#test:tool:76c25a388f0b4e4e85ec4a0eb794a6b564a41f19f1f17a0f984c9409e38f1193
                // @lfy def/mcp/commands.lfy:test#test:tool:430d6d90db0096b765c313099b0425e48c0069b5c3a1f92aa82676371e14f7f3
                ToolArgument::new(
                    "filter",
                    "the words that narrow which tests run, given to the test command as TESTS; every test runs when left out",
                    "string",
                    false,
                ),
                ToolArgument::new("scope", SCOPE_ARGUMENT, "string", false),
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
                // `allowStubs` has a default, so it is not listed as required.
                ToolArgument::new(
                    "allowStubs",
                    "whether a test that fails only by reaching a stub counts as passing, given to the command as ALLOW_STUBS; false when left out",
                    "boolean",
                    false,
                ),
            ],
            // @lfy def/mcp/commands.lfy:test#test:tool:c3ee2fa9249263228e75266d415288016fb367dd3e42ea7111abb95b2753f842
            // @lfy def/mcp/commands.lfy:test#test:tool:21a65a75a3ed0d3332a816d5fc81762a4aaafa8216d19d4536b1c82ba9fb2ac9
            // @lfy def/mcp/commands.lfy:test#test:tool:3073281f62f23eaf3e61e93368285720562153141f52e5c0ef0ed408a1fe7983
            // @lfy def/mcp/commands.lfy:test#test:tool:7d2fef009ba6dc46b09fa7aceab547155137968b1e8ed074aad0e0c693ab4950
            // @lfy def/mcp/commands.lfy:test#test:tool:805100fc5d20dfd5b05cb25d4737aec89a63dd10d2c9088ca35088065481d956
            // @lfy def/mcp/commands.lfy:test#test:tool:8e29089986847bb9e3dd6c6eb1b1d87ac2091a5acb94f321648a76498b553f68
            // @lfy def/mcp/commands.lfy:test#test:tool:237ea91ba542b9d07f5394b8c65fc2360ee924a487193374c9ed0fd5796a2d10
            // @lfy def/mcp/commands.lfy:test#test:tool:5c20d0aa955cefbb611028a3bb9e2be9b761f1036511d6ba89ffe31082eb4015
            |session, arguments| {
                test(
                    session,
                    arguments.optional("filter"),
                    arguments.optional("scope"),
                    arguments.optional("target"),
                    truth(arguments.optional("allowStubs"), false),
                )
            },
        ),
        // @lfy def/mcp/commands.lfy:lint#lint:tool:6e14d79b29847057cee32aad56ac37159ec40c848c8cb9346ffbcca0cf4ce022
        // @lfy def/mcp/commands.lfy:lint#lint:tool:f0044ba556ab8e00066360dce81c2a584d6ad207e30d0233e25d4294fde9e8a5
        // @lfy def/mcp/commands.lfy:lint#lint:tool:9f1b5c7200bd49c99da2c72dd74e0589a75980b35e4b797ecc4b538e18b380a1
        // @lfy def/mcp/commands.lfy:lint#lint:tool:fada2d3544a4caf7440370d2fdff08fd8d46235a4307ed27758f2aa426047db5
        Registered::new(
            "lint",
            "Run the target's lint command in this run's directory, narrowed to a scope when one is given, and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/commands.lfy:lint#lint:tool:f23edb098a47aff31e4aaab809a03e3db1a479cda385597dafd4ffca5fc96a43
            vec![
                // @lfy def/mcp/commands.lfy:lint#lint:tool:9293b2f58b28f6819e289105b3ffdfeddf57cb0b003b969596d58a7cf3cedeab
                // @lfy def/mcp/commands.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
                // @lfy def/mcp/commands.lfy:lint#lint:tool:8716f81a78f82cfecc907c464ca05907aaaf0ae13cf0d25431b6c89f1e71e1d9
                // @lfy def/mcp/commands.lfy:lint#lint:tool:81afa3f50b86d1ed54b7bad2244850fb1e841e53d326e18b8f9188e1037bc426
                ToolArgument::new("scope", SCOPE_ARGUMENT, "string", false),
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/commands.lfy:lint#lint:tool:67dfb4c019b54e4680477b0d722fdb00b27b2f4a851b62346c48fa4720da1f48
            // @lfy def/mcp/commands.lfy:lint#lint:tool:b0159114c3f88d628e5b62b17ca6b8953b1359d225b4142388fce204476c3aec
            // @lfy def/mcp/commands.lfy:lint#lint:tool:d6bc657b557dd76141f7e3de7f7503ec53bf669098e3548458d88f8faea06f71
            // @lfy def/mcp/commands.lfy:lint#lint:tool:cdc7eba40cf4a3184b3cb1b0636dbb899a1baf5373f13039407ca6374e147178
            // @lfy def/mcp/commands.lfy:lint#lint:tool:aec85fdd371eb5db1dbc0cf90cdf13875cdc4061a7d1e2a567a6ed0e8575762d
            // @lfy def/mcp/commands.lfy:lint#lint:tool:3a17eeac12b98c995afdd2992499aa8cfd276689d7504545bff7f9090266ff44
            // @lfy def/mcp/commands.lfy:lint#lint:tool:37270271962317584d5e2d9e760a441d5cca8d15e1b084389ac22bbe5fd906cf
            // @lfy def/mcp/commands.lfy:lint#lint:tool:5f09fb0d33ecba7c22d7aee2684106b4d835e32c750478ccf6f5f781d36f716a
            |session, arguments| lint(session, arguments.optional("scope"), arguments.optional("target")),
        ),
        // @lfy def/mcp/commands.lfy:format#format:tool:a84a6f1ea99ee3c31c458952672bfa71b4b076970b0ea7e5f97fc4ca1ad3476d
        // @lfy def/mcp/commands.lfy:format#format:tool:fad2173ad6add7bf6e4a61983e9685caf2ffe4dfb3b1a4887c920667e347acee
        // @lfy def/mcp/commands.lfy:format#format:tool:c9525e9b70499077881113750f0011474b3ed51c8f839564643d93637cedd60c
        // @lfy def/mcp/commands.lfy:format#format:tool:b844517eed0302f020cc276c74d1c8e649070a57de01e61e1bc8194d3157f1a5
        Registered::new(
            "format",
            "Run the target's format check in this run's directory and say whether it passed, with its exit code and the end of its output",
            // @lfy def/mcp/commands.lfy:format#format:tool:6b1377a3f7b741d23cf4ee8f6be9b6e69d161c9b715fd8f55d0247578017bb6c
            vec![
                // @lfy def/mcp/commands.lfy:format#format:tool:6f7967f9d4f086abae04ce989ce1d49bea43882ec4bbb31e34aee7bf9417d481
                // @lfy def/mcp/commands.lfy:format#format:tool:c7c05422e084fd79bb583a5aa90b70d947898019100fe9d2bd60b4fe23a540f2
                // @lfy def/mcp/commands.lfy:format#format:tool:97215d50932f95a6114e77ea1b0963c2a0c4806cae35d0b58fa00664bb7a8647
                // @lfy def/mcp/commands.lfy:format#format:tool:41a181e4de5b31903fd19b4fec451a4d3982908e194189ef3b45c691280dd06b
                ToolArgument::new("target", TARGET_ARGUMENT, "string", false),
            ],
            // @lfy def/mcp/commands.lfy:format#format:tool:c59813097edc6cf5ab13b506118db9446fd32c0da7b14d873b35f77bc6c773ae
            // @lfy def/mcp/commands.lfy:format#format:tool:ded962a70404db26d92fd2f1c148048bf282f6f74294da88a4ad6ff019d80e2f
            // @lfy def/mcp/commands.lfy:format#format:tool:54223274bff6ab3fc913d294ebef77a618b91dd99298cb2ba3fb8f0b745a64b8
            // @lfy def/mcp/commands.lfy:format#format:tool:8dd8b98f33f85088cb303af3a244f717653a87a913537f313cbc1a7d1975a5a4
            // @lfy def/mcp/commands.lfy:format#format:tool:b2c97631e6993afc21bc88c7f3e05498172f0153aab0eae0bfaeb5e964e70fc4
            // @lfy def/mcp/commands.lfy:format#format:tool:5f23b74ea1ea2db877ac4897e58e6eb22a1b830eda76d071948abb3c45aedf91
            // @lfy def/mcp/commands.lfy:format#format:tool:71b1b0cad7054dac9422dcee77e95c91d38e3b49363a05a35ea51aceb419d7dd
            // @lfy def/mcp/commands.lfy:format#format:tool:78b9d6daad5e2b3f5ccc2f0e0f4b7170c43a9c9ea683740b3770712eeddd69fd
            |session, arguments| format_check(session, arguments.optional("target")),
        ),
    ]
}

/// What the `scope` argument of a command tool says, as the fns that take one describe it.
// @lfy def/mcp/commands.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
// @lfy def/mcp/commands.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
// @lfy def/mcp/commands.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
const SCOPE_ARGUMENT: &str = "the part of the outputs to run on, as the target's ecosystem names it, such as a package for cargo; given to the command as SCOPE; all of them when left out";

/// What the `target` argument of a command tool says, as the fns that take one describe it.
// @lfy def/mcp/commands.lfy:build#build:tool:cc821fff828717e6310df1103a08b0267f91fc98bbc71bc89165e1224840cb87
// @lfy def/mcp/commands.lfy:test#test:tool:91cfa553c86897863866df4f70a520da5ab64eb9facaea1c00b2c552ee52e2da
// @lfy def/mcp/commands.lfy:lint#lint:tool:8415b8b106d669fec27102fd04507c3d814b919d8bcf79f8e7d2650f5fb4faee
// @lfy def/mcp/commands.lfy:format#format:tool:c7c05422e084fd79bb583a5aa90b70d947898019100fe9d2bd60b4fe23a540f2
const TARGET_ARGUMENT: &str = "the identifier of the target; the target of the session's unit, or the only target, when left out";

/// A boolean argument read back from the text it was spelled as, with what its parameter
/// defaults to when a call leaves it out.
// @lfy def/mcp/read.lfy:output
fn truth(text: Option<&str>, default: bool) -> bool {
    text.map_or(default, |text| text == "true")
}

/// A number argument that counts things — a line, an index, or a length — read back from
/// the text it was spelled as.
// @lfy def/mcp/read.lfy:source
fn counted(text: &str, name: &str) -> Result<usize, String> {
    let value: f64 = text
        .parse()
        .map_err(|_| format!("the argument {name} must be a number"))?;
    if value.fract() != 0.0 || value < 1.0 {
        return Err(format!("the argument {name} must be a whole number, counting from 1"));
    }
    Ok(value as usize)
}

/// A number argument read back from the text it was spelled as, whatever number it is, so
/// that the fn it is given to says what is wrong with a number it cannot use.
// @lfy def/mcp/compiling.lfy:mark
fn numeric(text: &str, name: &str) -> Result<f64, String> {
    text.parse().map_err(|_| format!("the argument {name} must be a number"))
}
