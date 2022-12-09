pub const INDEX_FILENAME: &str = "index.html";
pub const DETAIL_FILENAME: &str = "detail.html";
pub const INDEX_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Report</title>
     <style>

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

        table {
		  table-layout: fixed;
		}
		
        .error {
            background-color: #FF4646 !important;
        }
        
        h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		.container {
			padding:10px;
		}

    </style>
    <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
</head>
<body>

<div id="accordion">
{% for rule_report in rule_results %}
	<h3>{{ rule_report.rule.name }}</h3>
	<div class="container">
	<table class="report">
		<thead>
		<tr>
			<th>Nominal</th>
			<th>Actual</th>
			<th></th>
		</tr>
		</thead>
		<tbody>
			{% for file in rule_report.compare_results %}
				<tr {% if file.is_error %} class="error" {% endif %}>
					<td>{{ file.nominal }}</td>
					<td>{{ file.actual }}</td>
					<td>
						{% if file.detail_path %}
							<a href="./{{ rule_report.rule.name }}/{{ file.detail_path }}/{{ detail_filename }}">View Detail(s)</a>
						{% endif %}
					</td>
				</tr>
			{% endfor %}
		</tbody>
	</table>
	</div>
{% endfor %}
</div>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script
  src="https://code.jquery.com/ui/1.13.2/jquery-ui.min.js"
  integrity="sha256-lSjKY0/srUM9BE3dPm+c4fBo1dky2v27Gdjm2uoZaL0="
  crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('.report');
    });
    
    $(function() {
        $( "#accordion" ).accordion();
    });
</script>
</body>
</html>
"###;

pub const PLAIN_TEXT_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"###;

pub const PLAIN_IMAGE_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

        table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
            <tr>
                <td>{{ error }}</td>
            </tr>
    </tbody>
</table>

<p>
<h3>Nominal:</h3>
<img src="./{{ nominal_image }}" />
</p>

<p>
<h3>Actual:</h3>
<img src="./{{ actual_image }}" />
</p>

<p>
<h3>Diff:</h3>
<img src="./{{ diff_image }}" />
</p>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"###;

pub const PLAIN_CSV_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }
        
        table {
		  table-layout: fixed;
		}


		.actual {
			color: #0d6efdf0;
		}
		
		.diffs {
			color: #FF4646;
			font-size:12px;
		}
		
		table.dataTable {
			border: 1px solid #999999;
		}
		
		table.dataTable th:not(:last-child), table.dataTable td:not(:last-child) {
			border-right: 1px solid #999999;
		}

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
	    <th>Row</th>
		{% for col in headers %}
			<th>
				{{ col.actual_value }}
				{% if col.nominal_value != col.actual_value %}
					<div class="actual"> nominal: {{ col.nominal_value }}</span>
				{% endif %}
				{% for diff in col.diffs %}
					<div class="diffs">{{ diff }}</div>
				{% endfor %}
			</th>
		{% endfor %}
    </tr>
    </thead>
    <tbody>
        {% for cols in rows %}
            <tr>
            	<td>{{ loop.index }}</td>
            	{% for col in cols %}
					<td>
					{{ col.actual_value }}
					{% if col.nominal_value != col.actual_value %}
						<div class="actual">nominal: {{ col.nominal_value }}</span>
					{% endif %}
					{% for diff in col.diffs %}
						<div class="diffs">{{ diff }}</div>
					{% endfor %}
                	 </td>
                {% endfor %}
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report', {
			lengthMenu: [ [5, 10, 25, 50, -1], [5, 10, 25, 50, "All"] ],
			iDisplayLength: -1
		});
    });
</script>

</body>
</html>
"###;

pub const PLAIN_PDF_DETAIL_TEMPLATE: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>

     <style>

		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

        .helper {
        	color:orange;
        	font-weight:bold;
        }

		.helper a {
			color:orange;
		}

		.has_diff {
			color: #0d6efdf0;
		}

		.has_error {
			color:red;
		}

		#compare th {
			text-align:left;
			background-color: #cccccc;
			padding:10px;
		}

		#compare td:first-child {
			border-right: 1px solid black;
		}

		table#compare {
			border:1px solid grey;
		}

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<div class="helper">
This is for viewing only.
The extracted exact text can be downloaded here: <a href="./{{ nominal_extracted_filename }}">nominal</a> and <a href="./{{ actual_extracted_filename }}">actual</a>
</div>
<table id="compare">
	<thead>
		<tr>
			<th></th>
			<th>Nominal</th>
			<th>Actual</th>
		</tr>
	</thead>
	<tbody>
	{% for line in combined_lines %}
		<tr>
			<td>{{ loop.index }}</td>
			<td>{{ line.nominal_value|safe }}</td>
			<td>
				{% if line.diffs|length > 0 %}
					<span class="has_error">{{ line.actual_value|safe }}</span>
				{% elif line.actual_value != line.nominal_value %}
					<span class="has_diff">{{ line.actual_value|safe }}</span>
				{% else %}
					{{ line.actual_value|safe }}
				{% endif %}
			</td>
		</tr>
	{% endfor %}
	</tbody>
</table>

<br>
<br>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"###;
